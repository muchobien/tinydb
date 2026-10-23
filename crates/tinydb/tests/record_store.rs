use std::collections::HashSet;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tinydb::storage::{MAX_RECORD_SIZE, Pager, Record, RecordId, RecordStore, Value};

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tinydb-record-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&dir).unwrap();
        Self {
            path: dir.join("records.db"),
            dir,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn text(value: &str) -> Record {
    Record::new(vec![Value::Text(value.into())])
}

fn sample(index: usize) -> Record {
    Record::new(vec![
        Value::Integer(index as i64),
        Value::Text(format!("row {index}: 🌍 {}", "x".repeat(20 + index % 80))),
        Value::Null,
    ])
}

fn assert_error<T>(result: io::Result<T>, expected: ErrorKind) {
    match result {
        Ok(_) => panic!("expected {expected:?}, got success"),
        Err(error) => assert_eq!(error.kind(), expected, "{error}"),
    }
}

#[test]
fn stores_thousands_of_variable_length_records_across_pages_and_reopening() {
    let db = TestDb::new();
    let mut store = RecordStore::create(&db.path).unwrap();
    let mut records = Vec::new();
    for index in 0..3000 {
        let record = sample(index);
        let id = store.insert(&record).unwrap();
        records.push((id, Some(record)));
    }
    assert!(store.page_count() > 1);
    let original_pages = store.page_count();
    let old_ids: HashSet<_> = records.iter().map(|(id, _)| *id).collect();
    assert_eq!(old_ids.len(), 3000);
    for (index, (id, expected)) in records.iter_mut().enumerate() {
        if index % 7 == 0 {
            store.delete(*id).unwrap();
            *expected = None;
        } else if index % 11 == 0 {
            let updated = text("updated");
            store.update(*id, &updated).unwrap();
            *expected = Some(updated);
        }
    }
    // Space reclaimed across earlier pages should accept these new records.
    for index in 0..100 {
        let record = sample(index);
        let id = store.insert(&record).unwrap();
        assert!(!old_ids.contains(&id), "a retired ID was reused");
        records.push((id, Some(record)));
    }
    assert_eq!(store.page_count(), original_pages);
    store.sync().unwrap();
    drop(store);

    let mut store = RecordStore::open(&db.path).unwrap();
    for (id, expected) in records.into_iter().rev() {
        match expected {
            Some(record) => assert_eq!(store.read(id).unwrap(), record),
            None => assert_error(store.read(id), ErrorKind::NotFound),
        }
    }
}

#[test]
fn create_and_open_preserve_existing_files_and_require_valid_storage() {
    let db = TestDb::new();
    assert_error(RecordStore::open(&db.path), ErrorKind::NotFound);
    let store = RecordStore::create(&db.path).unwrap();
    assert_eq!(store.page_count(), 0);
    store.sync().unwrap();
    drop(store);
    let before = fs::read(&db.path).unwrap();
    assert_error(RecordStore::create(&db.path), ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&db.path).unwrap(), before);
    assert_eq!(RecordStore::open(&db.path).unwrap().page_count(), 0);
}

#[test]
fn oversized_inserts_do_not_allocate_pages() {
    let db = TestDb::new();
    let mut store = RecordStore::create(&db.path).unwrap();
    assert_error(
        store.insert(&text(&"x".repeat(MAX_RECORD_SIZE))),
        ErrorKind::StorageFull,
    );
    assert_eq!(store.page_count(), 0);
    let largest = text(&"y".repeat(MAX_RECORD_SIZE - 7));
    assert_eq!(largest.encode().unwrap().len(), MAX_RECORD_SIZE);
    let id = store.insert(&largest).unwrap();
    assert_eq!(
        id,
        RecordId {
            page_id: 1,
            slot_id: 0
        }
    );
    assert_eq!(store.read(id).unwrap(), largest);
    let second = store.insert(&Record::new(vec![])).unwrap();
    assert_eq!(second.page_id, 2);
    assert_eq!(store.read(second).unwrap(), Record::new(vec![]));
}

#[test]
fn updates_preserve_ids_and_reject_growth_without_changing_disk_bytes() {
    let db = TestDb::new();
    let mut store = RecordStore::create(&db.path).unwrap();
    let first = store.insert(&text(&"a".repeat(2000))).unwrap();
    let second = store.insert(&text(&"b".repeat(2000))).unwrap();
    assert_eq!(first.page_id, second.page_id);
    let before = fs::read(&db.path).unwrap();
    assert_error(
        store.update(first, &text(&"c".repeat(2100))),
        ErrorKind::StorageFull,
    );
    assert_error(
        store.update(first, &text(&"c".repeat(MAX_RECORD_SIZE))),
        ErrorKind::StorageFull,
    );
    assert_eq!(fs::read(&db.path).unwrap(), before);
    assert_eq!(store.read(first).unwrap(), text(&"a".repeat(2000)));
    assert_eq!(store.read(second).unwrap(), text(&"b".repeat(2000)));

    store.delete(second).unwrap();
    let grown = text(&"c".repeat(2100));
    store.update(first, &grown).unwrap();
    let new_id = store.insert(&text("new")).unwrap();
    assert_eq!(new_id.page_id, first.page_id);
    assert_ne!(new_id, second);
    assert_error(store.read(second), ErrorKind::NotFound);
    store.sync().unwrap();
    drop(store);
    let mut store = RecordStore::open(&db.path).unwrap();
    assert_eq!(store.read(first).unwrap(), grown);
    assert_error(store.read(second), ErrorKind::NotFound);
    assert_eq!(store.read(new_id).unwrap(), text("new"));
}

#[test]
fn rejects_invalid_page_ids_missing_slots_and_repeated_deletes() {
    let db = TestDb::new();
    let mut store = RecordStore::create(&db.path).unwrap();
    let id = store.insert(&text("keep")).unwrap();
    let before = fs::read(&db.path).unwrap();
    for page_id in [0, 2, u64::MAX] {
        let missing = RecordId {
            page_id,
            slot_id: 0,
        };
        assert_error(store.read(missing), ErrorKind::InvalidInput);
        assert_error(store.update(missing, &text("bad")), ErrorKind::InvalidInput);
        assert_error(store.delete(missing), ErrorKind::InvalidInput);
    }
    for slot_id in [1, u16::MAX] {
        let missing = RecordId {
            page_id: 1,
            slot_id,
        };
        assert_error(store.read(missing), ErrorKind::NotFound);
        assert_error(store.update(missing, &text("bad")), ErrorKind::NotFound);
        assert_error(store.delete(missing), ErrorKind::NotFound);
    }
    assert_eq!(fs::read(&db.path).unwrap(), before);
    store.delete(id).unwrap();
    assert_error(store.delete(id), ErrorKind::NotFound);
    assert_error(store.update(id, &text("bad")), ErrorKind::NotFound);
    let next = store.insert(&text("new")).unwrap();
    assert_ne!(id, next);
    assert_error(store.read(id), ErrorKind::NotFound);
}

#[test]
fn open_rejects_zeroed_pages_and_invalid_stored_record_encodings() {
    let db = TestDb::new();
    let mut pager = Pager::create(&db.path).unwrap();
    pager.allocate_page().unwrap();
    drop(pager);
    assert_error(RecordStore::open(&db.path), ErrorKind::InvalidData);

    let db = TestDb::new();
    let mut store = RecordStore::create(&db.path).unwrap();
    store.insert(&text("valid")).unwrap();
    drop(store);
    let mut pager = Pager::open(&db.path).unwrap();
    let mut page = pager.read_page(1).unwrap();
    let offset = usize::from(u16::from_le_bytes(page[16..18].try_into().unwrap()));
    page[offset + 2] = 255; // structurally valid slot, unknown value tag
    pager.write_page(1, &page).unwrap();
    drop(pager);
    assert_error(RecordStore::open(&db.path), ErrorKind::InvalidData);
}

#[test]
fn record_writer_child_process() {
    let Some(path) = std::env::var_os("TINYDB_RECORD_TEST_CHILD_PATH") else {
        return;
    };
    let mut store = RecordStore::create(path).unwrap();
    let id = store.insert(&sample(0)).unwrap();
    store.delete(id).unwrap();
    let id = store.insert(&sample(1)).unwrap();
    assert_eq!(
        id,
        RecordId {
            page_id: 1,
            slot_id: 1
        }
    );
    store.update(id, &text("written in child")).unwrap();
    store.sync().unwrap();
}

#[test]
fn records_and_tombstones_persist_after_writer_process_exits() {
    let db = TestDb::new();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "record_writer_child_process", "--nocapture"])
        .env("TINYDB_RECORD_TEST_CHILD_PATH", &db.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut store = RecordStore::open(&db.path).unwrap();
    assert_error(
        store.read(RecordId {
            page_id: 1,
            slot_id: 0,
        }),
        ErrorKind::NotFound,
    );
    assert_eq!(
        store
            .read(RecordId {
                page_id: 1,
                slot_id: 1
            })
            .unwrap(),
        text("written in child")
    );
}
