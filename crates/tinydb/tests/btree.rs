use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind};
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tinydb::index::BPlusTree;
use tinydb::storage::{PAGE_SIZE, Pager, Record, RecordId, RecordStore, Value};

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tinydb-index-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&dir).unwrap();
        Self {
            path: dir.join("index.db"),
            dir,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn record_id(key: i64) -> RecordId {
    RecordId {
        page_id: key.unsigned_abs().saturating_add(1),
        slot_id: (key.unsigned_abs() % 1020) as u16,
    }
}

fn assert_error<T>(result: io::Result<T>, expected: ErrorKind) {
    match result {
        Ok(_) => panic!("expected {expected:?}, got success"),
        Err(error) => assert_eq!(error.kind(), expected, "{error}"),
    }
}

#[test]
fn creation_lookup_and_rejected_mutations_preserve_disk_bytes() {
    let db = TestDb::new();
    assert_error(BPlusTree::open(&db.path), ErrorKind::NotFound);
    let mut tree = BPlusTree::create(&db.path).unwrap();
    assert_eq!(tree.page_count(), 1);
    assert_eq!(tree.get(0).unwrap(), None);
    assert!(tree.range(..).unwrap().is_empty());
    for key in [i64::MIN, -1, 0, 1, i64::MAX] {
        tree.insert(key, record_id(key)).unwrap();
    }
    tree.sync().unwrap();
    let before = fs::read(&db.path).unwrap();
    assert_error(BPlusTree::create(&db.path), ErrorKind::AlreadyExists);
    assert_error(tree.insert(0, record_id(100)), ErrorKind::AlreadyExists);
    assert_error(tree.delete(2), ErrorKind::NotFound);
    assert_error(
        tree.insert(
            2,
            RecordId {
                page_id: 0,
                slot_id: 0,
            },
        ),
        ErrorKind::InvalidInput,
    );
    assert_eq!(fs::read(&db.path).unwrap(), before);
    drop(tree);
    let mut tree = BPlusTree::open(&db.path).unwrap();
    for key in [i64::MIN, -1, 0, 1, i64::MAX] {
        assert_eq!(tree.get(key).unwrap(), Some(record_id(key)));
    }
    assert_eq!(tree.get(2).unwrap(), None);
    assert_eq!(
        tree.range(i64::MIN..=i64::MIN).unwrap(),
        vec![(i64::MIN, record_id(i64::MIN))]
    );
    assert_eq!(tree.range((Excluded(i64::MAX), Unbounded)).unwrap(), vec![]);
    assert_eq!(tree.range(..=i64::MAX).unwrap().len(), 5);
}

#[test]
fn node_bytes_use_little_endian_fields_and_root_stays_on_page_one() {
    let db = TestDb::new();
    let mut tree = BPlusTree::create(&db.path).unwrap();
    tree.insert(
        -42,
        RecordId {
            page_id: 1234,
            slot_id: 567,
        },
    )
    .unwrap();
    drop(tree);
    let bytes = fs::read(&db.path).unwrap();
    let root = &bytes[PAGE_SIZE..2 * PAGE_SIZE];
    assert_eq!(&root[..4], b"BTL1");
    assert_eq!(&root[4..6], &1_u16.to_le_bytes());
    assert_eq!(&root[6..16], &[0; 10]);
    assert_eq!(&root[16..24], &(-42_i64).to_le_bytes());
    assert_eq!(&root[24..32], &1234_u64.to_le_bytes());
    assert_eq!(&root[32..34], &567_u16.to_le_bytes());
    assert!(root[34..].iter().all(|&byte| byte == 0));

    let mut tree = BPlusTree::open(&db.path).unwrap();
    tree.delete(-42).unwrap();
    for key in 0..227 {
        tree.insert(key, record_id(key)).unwrap();
    }
    assert_eq!(tree.page_count(), 3);
    drop(tree);
    let mut pager = Pager::open(&db.path).unwrap();
    let root = pager.read_page(1).unwrap();
    assert_eq!(&root[..4], b"BTI1");
    assert_eq!(&root[4..6], &1_u16.to_le_bytes());
    assert_eq!(&root[6..16], &[0; 10]);
    assert_eq!(&root[16..24], &3_u64.to_le_bytes());
    assert_eq!(&root[24..32], &113_i64.to_le_bytes());
    assert_eq!(&root[32..40], &2_u64.to_le_bytes());
    let left = pager.read_page(3).unwrap();
    assert_eq!(&left[8..16], &2_u64.to_le_bytes());
    assert_eq!(&left[4..6], &113_u16.to_le_bytes());
    let right = pager.read_page(2).unwrap();
    assert_eq!(&right[4..6], &114_u16.to_le_bytes());
    assert_eq!(&right[8..16], &[0; 8]);
}

#[test]
fn range_bounds_seek_across_leaf_splits_in_sorted_order() {
    let db = TestDb::new();
    let mut tree = BPlusTree::create(&db.path).unwrap();
    let expected: BTreeMap<_, _> = (-1500..1500).map(|key| (key * 2, record_id(key))).collect();
    for (&key, &record) in expected.iter().rev() {
        tree.insert(key, record).unwrap();
    }
    for lower in [
        Unbounded,
        Included(-4000),
        Included(-226),
        Excluded(-226),
        Included(-225),
        Included(0),
        Excluded(2998),
        Included(4000),
    ] {
        for upper in [
            Unbounded,
            Included(-4000),
            Included(-226),
            Excluded(-226),
            Included(225),
            Excluded(226),
            Included(226),
            Included(4000),
        ] {
            let wanted: Vec<_> = expected
                .iter()
                .filter(|&(&key, _)| {
                    let above = match lower {
                        Included(start) => key >= start,
                        Excluded(start) => key > start,
                        Unbounded => true,
                    };
                    let below = match upper {
                        Included(end) => key <= end,
                        Excluded(end) => key < end,
                        Unbounded => true,
                    };
                    above && below
                })
                .map(|(&key, &record)| (key, record))
                .collect();
            assert_eq!(
                tree.range((lower, upper)).unwrap(),
                wanted,
                "{lower:?}..{upper:?}"
            );
        }
    }
    drop(tree);
    let mut tree = BPlusTree::open(&db.path).unwrap();
    assert_eq!(
        tree.range(..).unwrap(),
        expected.into_iter().collect::<Vec<_>>()
    );
}

#[test]
fn mixed_mutations_match_a_reference_map_after_repeated_reopening() {
    let db = TestDb::new();
    let mut tree = BPlusTree::create(&db.path).unwrap();
    let mut expected = BTreeMap::new();
    for key in -2000..2000 {
        tree.insert(key, record_id(key)).unwrap();
        expected.insert(key, record_id(key));
    }
    let mut state = 0x273a_8b49_163c_8d55_u64;
    for step in 0..16000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let key = ((state >> 16) % 8000) as i64 - 4000;
        match state >> 62 {
            0 | 1 => {
                let record = record_id(step);
                if let std::collections::btree_map::Entry::Vacant(entry) = expected.entry(key) {
                    tree.insert(key, record).unwrap();
                    entry.insert(record);
                } else {
                    assert_error(tree.insert(key, record), ErrorKind::AlreadyExists);
                }
            }
            2 => match expected.remove(&key) {
                Some(record) => assert_eq!(tree.delete(key).unwrap(), record),
                None => assert_error(tree.delete(key), ErrorKind::NotFound),
            },
            _ => assert_eq!(tree.get(key).unwrap(), expected.get(&key).copied()),
        }
        if step % 1000 == 0 {
            tree.sync().unwrap();
            drop(tree);
            tree = BPlusTree::open(&db.path).unwrap();
            assert_eq!(
                tree.range(..).unwrap(),
                expected
                    .iter()
                    .map(|(&key, &record)| (key, record))
                    .collect::<Vec<_>>()
            );
        }
    }
    for (&key, &record) in &expected {
        assert_eq!(tree.get(key).unwrap(), Some(record));
    }
}

#[test]
fn three_level_trees_rebalance_contract_and_accept_new_inserts() {
    for descending in [false, true] {
        let db = TestDb::new();
        let mut tree = BPlusTree::create(&db.path).unwrap();
        for offset in 0..60000 {
            let key = if descending { 59999 - offset } else { offset };
            tree.insert(key, record_id(key)).unwrap();
        }
        tree.sync().unwrap();
        let pages = tree.page_count();
        assert!(pages > 255);
        drop(tree);
        let mut pager = Pager::open(&db.path).unwrap();
        let root = pager.read_page(1).unwrap();
        assert_eq!(&root[..4], b"BTI1");
        let child_id = u64::from_le_bytes(root[16..24].try_into().unwrap());
        assert_eq!(&pager.read_page(child_id).unwrap()[..4], b"BTI1");
        drop(pager);
        let mut tree = BPlusTree::open(&db.path).unwrap();
        let mut keys: Vec<i64> = (0..60000).collect();
        let mut state = 12345_u64;
        for index in (1..keys.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            keys.swap(index, (state as usize) % (index + 1));
        }
        if descending {
            keys.sort_unstable();
            keys.reverse();
        }
        for (index, key) in keys.into_iter().enumerate() {
            assert_eq!(tree.delete(key).unwrap(), record_id(key));
            if index % 5000 == 0 {
                tree.sync().unwrap();
                drop(tree);
                tree = BPlusTree::open(&db.path).unwrap();
                assert_eq!(tree.range(..).unwrap().len(), 59999 - index);
                assert_eq!(tree.get(key).unwrap(), None);
            }
        }
        assert_eq!(tree.page_count(), pages);
        assert!(tree.range(..).unwrap().is_empty());
        drop(tree);
        let mut pager = Pager::open(&db.path).unwrap();
        let root = pager.read_page(1).unwrap();
        assert_eq!(&root[..4], b"BTL1");
        assert!(root[4..].iter().all(|&byte| byte == 0));
        for page_id in 2..=pages {
            let page = pager.read_page(page_id).unwrap();
            assert_eq!(&page[..4], b"BTF1");
            assert!(page[4..].iter().all(|&byte| byte == 0));
        }
        drop(pager);
        let mut tree = BPlusTree::open(&db.path).unwrap();
        for key in 0..1000 {
            tree.insert(key, record_id(key)).unwrap();
        }
        drop(tree);
        assert_eq!(
            BPlusTree::open(&db.path).unwrap().range(..).unwrap().len(),
            1000
        );
    }
}

#[test]
fn malformed_pages_and_tree_relationships_are_rejected_on_open() {
    let db = TestDb::new();
    let mut tree = BPlusTree::create(&db.path).unwrap();
    for key in 0..500 {
        tree.insert(key, record_id(key)).unwrap();
    }
    drop(tree);
    let original = fs::read(&db.path).unwrap();
    let root_offset = PAGE_SIZE;
    let leaf_id = u64::from_le_bytes(
        original[root_offset + 16..root_offset + 24]
            .try_into()
            .unwrap(),
    ) as usize;
    let leaf_offset = leaf_id * PAGE_SIZE;
    let corruptions = [
        (root_offset, vec![0]),
        (root_offset + 4, u16::MAX.to_le_bytes().to_vec()),
        (root_offset + 6, vec![1]),
        (root_offset + 8, vec![1]),
        (root_offset + 16, 0_u64.to_le_bytes().to_vec()),
        (root_offset + 16, u64::MAX.to_le_bytes().to_vec()),
        (root_offset + 16, 1_u64.to_le_bytes().to_vec()),
        (root_offset + 24, 112_i64.to_le_bytes().to_vec()),
        (root_offset + 32, (leaf_id as u64).to_le_bytes().to_vec()),
        (root_offset + PAGE_SIZE - 1, vec![1]),
        (leaf_offset + 4, 112_u16.to_le_bytes().to_vec()),
        (leaf_offset + 8, 0_u64.to_le_bytes().to_vec()),
        (leaf_offset + 8, u64::MAX.to_le_bytes().to_vec()),
        (leaf_offset + 8, 1_u64.to_le_bytes().to_vec()),
        (leaf_offset + 16, 1_i64.to_le_bytes().to_vec()),
        (leaf_offset + 24, 0_u64.to_le_bytes().to_vec()),
        (leaf_offset + PAGE_SIZE - 1, vec![1]),
    ];
    for (offset, replacement) in corruptions {
        let mut bytes = original.clone();
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
        fs::write(&db.path, &bytes).unwrap();
        assert_error(BPlusTree::open(&db.path), ErrorKind::InvalidData);
    }
    fs::write(&db.path, &original).unwrap();
    let mut pager = Pager::open(&db.path).unwrap();
    pager.allocate_page().unwrap();
    drop(pager);
    assert_error(BPlusTree::open(&db.path), ErrorKind::InvalidData);

    let db = TestDb::new();
    drop(Pager::create(&db.path).unwrap());
    assert_error(BPlusTree::open(&db.path), ErrorKind::InvalidData);
    let db = TestDb::new();
    let mut heap = RecordStore::create(&db.path).unwrap();
    heap.insert(&Record::new(vec![Value::Integer(1)])).unwrap();
    drop(heap);
    assert_error(BPlusTree::open(&db.path), ErrorKind::InvalidData);
}

#[test]
fn index_writer_child_process() {
    let Some(path) = std::env::var_os("TINYDB_INDEX_TEST_CHILD_PATH") else {
        return;
    };
    let mut tree = BPlusTree::create(path).unwrap();
    for key in 0..1000 {
        tree.insert(key, record_id(key)).unwrap();
    }
    for key in 0..900 {
        tree.delete(key).unwrap();
    }
    tree.sync().unwrap();
}

#[test]
fn index_survives_writer_process_exit_and_resolves_real_heap_records() {
    let db = TestDb::new();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "index_writer_child_process", "--nocapture"])
        .env("TINYDB_INDEX_TEST_CHILD_PATH", &db.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut tree = BPlusTree::open(&db.path).unwrap();
    assert_eq!(
        tree.range(..).unwrap(),
        (900..1000)
            .map(|key| (key, record_id(key)))
            .collect::<Vec<_>>()
    );

    let db = TestDb::new();
    let heap_path = db.dir.join("heap.db");
    let mut heap = RecordStore::create(&heap_path).unwrap();
    let mut tree = BPlusTree::create(&db.path).unwrap();
    let mut expected = Vec::new();
    for key in (0..500).rev() {
        let record = Record::new(vec![Value::Integer(key), Value::Text(format!("row {key}"))]);
        let id = heap.insert(&record).unwrap();
        tree.insert(key, id).unwrap();
        expected.push(record);
    }
    tree.sync().unwrap();
    heap.sync().unwrap();
    drop(tree);
    drop(heap);
    let mut heap = RecordStore::open(&heap_path).unwrap();
    let mut tree = BPlusTree::open(&db.path).unwrap();
    for ((key, id), record) in tree
        .range(..)
        .unwrap()
        .into_iter()
        .zip(expected.into_iter().rev())
    {
        assert_eq!(heap.read(id).unwrap(), record);
        assert_eq!(tree.get(key).unwrap(), Some(id));
    }
}
