use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tinydb::storage::{PAGE_SIZE, Page, Pager};

/// Each test owns its directory, including when Cargo runs tests concurrently.
struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "tinydb-test-{}-{}-{}",
            std::process::id(),
            timestamp.as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("test.db");
        Self { dir, path }
    }

    fn create(&self) -> Pager {
        Pager::create(&self.path).unwrap()
    }

    fn edit(&self, offset: u64, bytes: &[u8]) {
        let mut file = OpenOptions::new().write(true).open(&self.path).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(bytes).unwrap();
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn assert_error<T>(result: io::Result<T>, expected: ErrorKind) {
    match result {
        Ok(_) => panic!("expected {expected:?}, got success"),
        Err(error) => assert_eq!(error.kind(), expected, "{error}"),
    }
}

fn pattern(seed: u8) -> Page {
    std::array::from_fn(|index| (index as u8).wrapping_add(seed))
}

#[test]
fn creates_an_exact_little_endian_header() {
    let db = TestDb::new();
    let pager = db.create();
    assert_eq!(pager.page_count(), 0);
    pager.sync().unwrap();
    drop(pager);

    let bytes = fs::read(&db.path).unwrap();
    assert_eq!(bytes.len(), PAGE_SIZE);
    assert_eq!(
        &bytes[..24],
        &[
            0x54, 0x49, 0x4e, 0x59, 0x44, 0x42, 0x00, 0x00, // magic
            0x01, 0x00, 0x00, 0x00, // version 1
            0x00, 0x10, 0x00, 0x00, // 4096 bytes
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // no data pages
        ]
    );
    assert!(bytes[24..].iter().all(|&byte| byte == 0));
    assert_eq!(Pager::open(&db.path).unwrap().page_count(), 0);
}

#[test]
fn create_never_overwrites_an_existing_file() {
    let db = TestDb::new();
    fs::write(&db.path, b"keep this file").unwrap();
    assert_error(Pager::create(&db.path), ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&db.path).unwrap(), b"keep this file");
}

#[test]
fn open_does_not_create_a_missing_database() {
    let db = TestDb::new();
    assert_error(Pager::open(&db.path), ErrorKind::NotFound);
    assert!(!db.path.exists());
}

#[test]
fn allocates_zeroed_pages_with_stable_ids_and_offsets() {
    let db = TestDb::new();
    let mut pager = db.create();
    for id in 1..=3 {
        assert_eq!(pager.allocate_page().unwrap(), id);
        assert_eq!(pager.page_count(), id);
        assert_eq!(pager.read_page(id).unwrap(), [0; PAGE_SIZE]);
        pager.write_page(id, &pattern(id as u8)).unwrap();
    }
    pager.sync().unwrap();
    drop(pager);

    let bytes = fs::read(&db.path).unwrap();
    assert_eq!(bytes.len(), 4 * PAGE_SIZE);
    assert_eq!(&bytes[16..24], &3_u64.to_le_bytes());
    for id in 1..=3 {
        assert_eq!(
            &bytes[id * PAGE_SIZE..(id + 1) * PAGE_SIZE],
            &pattern(id as u8)
        );
    }

    let mut pager = Pager::open(&db.path).unwrap();
    assert_eq!(pager.page_count(), 3);
    assert_eq!(pager.allocate_page().unwrap(), 4);
    assert_eq!(pager.read_page(4).unwrap(), [0; PAGE_SIZE]);
    for id in 1..=3 {
        assert_eq!(pager.read_page(id).unwrap(), pattern(id as u8));
    }
}

#[test]
fn overwriting_one_page_preserves_its_neighbors_and_file_length() {
    let db = TestDb::new();
    let mut pager = db.create();
    for seed in 1..=3 {
        let id = pager.allocate_page().unwrap();
        pager.write_page(id, &pattern(seed)).unwrap();
    }
    pager.write_page(2, &pattern(99)).unwrap();
    pager.sync().unwrap();
    drop(pager);

    let mut pager = Pager::open(&db.path).unwrap();
    assert_eq!(pager.read_page(3).unwrap(), pattern(3));
    assert_eq!(pager.read_page(1).unwrap(), pattern(1));
    assert_eq!(pager.read_page(2).unwrap(), pattern(99));
    assert_eq!(pager.page_count(), 3);
    assert_eq!(
        fs::metadata(&db.path).unwrap().len(),
        (4 * PAGE_SIZE) as u64
    );
}

#[test]
fn rejects_metadata_and_unallocated_page_ids_without_changing_the_file() {
    let db = TestDb::new();
    let mut pager = db.create();
    assert_error(pager.read_page(1), ErrorKind::InvalidInput);
    assert_eq!(pager.allocate_page().unwrap(), 1);
    let before = fs::read(&db.path).unwrap();
    for id in [0, 2, u64::MAX] {
        assert_error(pager.read_page(id), ErrorKind::InvalidInput);
        assert_error(pager.write_page(id, &pattern(1)), ErrorKind::InvalidInput);
    }
    assert_eq!(fs::read(&db.path).unwrap(), before);
}

#[test]
fn rejects_invalid_magic_version_page_size_and_reserved_bytes() {
    for (offset, bytes) in [
        (0, b"NOTADB!!".to_vec()),
        (8, 2_u32.to_le_bytes().to_vec()),
        (8, 1_u32.to_be_bytes().to_vec()),
        (12, 8192_u32.to_le_bytes().to_vec()),
        (24, vec![1]),
        ((PAGE_SIZE - 1) as u64, vec![1]),
    ] {
        let db = TestDb::new();
        drop(db.create());
        db.edit(offset, &bytes);
        assert_error(Pager::open(&db.path), ErrorKind::InvalidData);
    }
}

#[test]
fn rejects_empty_and_truncated_metadata_pages() {
    for length in [0, 1, 23, 24, PAGE_SIZE - 1] {
        let db = TestDb::new();
        drop(db.create());
        OpenOptions::new()
            .write(true)
            .open(&db.path)
            .unwrap()
            .set_len(length as u64)
            .unwrap();
        assert_error(Pager::open(&db.path), ErrorKind::InvalidData);
    }
}

#[test]
fn rejects_missing_pages_partial_pages_and_trailing_bytes() {
    for length in [
        PAGE_SIZE,
        2 * PAGE_SIZE - 1,
        2 * PAGE_SIZE + 1,
        3 * PAGE_SIZE,
    ] {
        let db = TestDb::new();
        let mut pager = db.create();
        pager.allocate_page().unwrap();
        drop(pager);
        OpenOptions::new()
            .write(true)
            .open(&db.path)
            .unwrap()
            .set_len(length as u64)
            .unwrap();
        assert_error(Pager::open(&db.path), ErrorKind::InvalidData);
    }
}

#[test]
fn rejects_incorrect_and_overflowing_page_counts() {
    for count in [1, u64::MAX / PAGE_SIZE as u64, u64::MAX] {
        let db = TestDb::new();
        drop(db.create());
        db.edit(16, &count.to_le_bytes());
        assert_error(Pager::open(&db.path), ErrorKind::InvalidData);
    }
}

// The parent launches this exact test in a separate process. In a normal test
// run there is no fixture path, so this entry point has nothing to write.
#[test]
fn child_process_writes_database() {
    let Some(path) = std::env::var_os("TINYDB_TEST_CHILD_PATH") else {
        return;
    };
    let mut pager = Pager::create(path).unwrap();
    assert_eq!(pager.allocate_page().unwrap(), 1);
    assert_eq!(pager.allocate_page().unwrap(), 2);
    pager.write_page(1, &pattern(17)).unwrap();
    pager.write_page(2, &pattern(231)).unwrap();
    pager.sync().unwrap();
}

#[test]
fn persists_pages_after_the_writer_process_exits() {
    let db = TestDb::new();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_process_writes_database", "--nocapture"])
        .env("TINYDB_TEST_CHILD_PATH", &db.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child process failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut pager = Pager::open(&db.path).unwrap();
    assert_eq!(pager.page_count(), 2);
    assert_eq!(pager.read_page(2).unwrap(), pattern(231));
    assert_eq!(pager.read_page(1).unwrap(), pattern(17));
    assert_eq!(pager.allocate_page().unwrap(), 3);
}
