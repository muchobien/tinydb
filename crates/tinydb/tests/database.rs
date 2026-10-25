use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tinydb::execution::AccessPath;
use tinydb::index::BPlusTree;
use tinydb::sql::{Output, Session, parse};
use tinydb::storage::{Record, RecordId, RecordStore, Value};
use tinydb::{Database, DatabaseError};

struct TestDb {
    root: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "tinydb-database-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            path: root.join("database"),
            root,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn snapshot(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

fn rows(database: &mut Database, source: &str) -> Vec<Vec<Value>> {
    let Output::Rows { rows, .. } = database.query(source).unwrap() else {
        panic!("expected rows");
    };
    rows
}

fn assert_storage_error<T>(result: Result<T, DatabaseError>, kind: ErrorKind) {
    match result {
        Err(DatabaseError::Storage(error)) => assert_eq!(error.kind(), kind, "{error}"),
        Err(error) => panic!("unexpected {error}"),
        Ok(_) => panic!("expected storage error"),
    }
}

#[test]
fn persistent_crud_matches_memory_execution_after_every_reopen() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    let mut session = Session::default();
    let source = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, team INTEGER);
        CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE notes (name TEXT PRIMARY KEY, body TEXT);
        INSERT INTO teams VALUES (10, 'Blue'), (20, 'Green');
        INSERT INTO users VALUES (3, 'Charlie', 20), (1, 'Alice', 10), (2, 'Bob', 10);
        INSERT INTO notes VALUES ('one', 'x');
        UPDATE users SET name = 'Alice García' WHERE users.id = 1;
        UPDATE users SET id = 22 WHERE id = 2;
        UPDATE notes SET body = 'a much longer replacement value';
        DELETE FROM users WHERE id = 3;
        SELECT * FROM users ORDER BY id;
        SELECT u.name, t.name FROM users AS u JOIN teams AS t ON u.team = t.id ORDER BY u.id;
        SELECT team, COUNT(*) AS n, MIN(name) FROM users GROUP BY team ORDER BY n;
        SELECT * FROM notes;
        DELETE FROM notes;
        SELECT COUNT(*) FROM notes;";
    for statement in parse(source).unwrap() {
        assert_eq!(
            database.execute_statement(&statement).unwrap(),
            session.execute(&statement).unwrap()
        );
        drop(database);
        database = Database::open(&db.path).unwrap();
    }
    assert_eq!(
        rows(&mut database, "SELECT id FROM users ORDER BY id"),
        vec![vec![Value::Integer(1)], vec![Value::Integer(22)]]
    );
    drop(database);
    let mut heap = RecordStore::open(db.path.join("table-1.db")).unwrap();
    let records = heap.scan().unwrap();
    assert_eq!(records.len(), 2);
    drop(heap);
    let mut index = BPlusTree::open(db.path.join("index-1.db")).unwrap();
    for (key, id) in index.range(..).unwrap() {
        assert!(
            records
                .iter()
                .any(|(record_id, record)| *record_id == id
                    && record.values[0] == Value::Integer(key))
        );
    }
}

#[test]
fn primary_key_plans_and_index_results_match_forced_sequential_scans() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database
        .execute("CREATE TABLE t (id INTEGER PRIMARY KEY, score INTEGER);")
        .unwrap();
    let values = (-600..600)
        .rev()
        .map(|key| format!("({key}, {})", key % 7))
        .collect::<Vec<_>>()
        .join(",");
    database
        .execute(&format!("INSERT INTO t VALUES {values}"))
        .unwrap();
    database
        .execute("INSERT INTO t VALUES (-9223372036854775808, 0), (9223372036854775807, 0);")
        .unwrap();
    for predicate in [
        "id = 10",
        "id > -500 AND id <= 200",
        "id >= 20 AND 50 > id",
        "10 < id AND id < 100 AND score = 1",
        "id >= 10 AND id > 10 AND id < 12",
        "id >= 100 AND id < -100",
        "id > 10 AND id <= 10",
        "id = -9223372036854775808",
        "id >= 9223372036854775807",
        "id < -9223372036854775808",
    ] {
        let source = format!("SELECT id, score FROM t WHERE {predicate} ORDER BY id");
        let forced = format!("SELECT id, score FROM t WHERE NOT NOT ({predicate}) ORDER BY id");
        assert!(
            matches!(
                database.plan(&source).unwrap()[0].access,
                AccessPath::PrimaryKey { .. }
            ),
            "{predicate}"
        );
        assert_eq!(
            database.plan(&forced).unwrap()[0].access,
            AccessPath::Sequential
        );
        assert_eq!(
            database.query(&source).unwrap(),
            database.query(&forced).unwrap(),
            "{predicate}"
        );
    }
    assert_eq!(
        database
            .plan("SELECT * FROM t WHERE id >= 10 AND id > 10 AND 20 >= id")
            .unwrap()[0]
            .access,
        AccessPath::PrimaryKey {
            lower: Excluded(10),
            upper: Included(20)
        }
    );
    assert_eq!(
        database.plan("SELECT * FROM t WHERE 1 = id").unwrap()[0].access,
        AccessPath::PrimaryKey {
            lower: Included(1),
            upper: Included(1)
        }
    );
    assert_eq!(
        database
            .plan("SELECT * FROM t AS a WHERE a.id > 1")
            .unwrap()[0]
            .access,
        AccessPath::PrimaryKey {
            lower: Excluded(1),
            upper: Unbounded
        }
    );
    for predicate in ["id != 1", "id = 1 OR id = 2", "score = 1", "id IS NULL"] {
        assert_eq!(
            database
                .plan(&format!("SELECT * FROM t WHERE {predicate}"))
                .unwrap()[0]
                .access,
            AccessPath::Sequential
        );
    }
    database
        .execute("UPDATE t SET id = 9999 WHERE id = 10; DELETE FROM t WHERE id < -500;")
        .unwrap();
    drop(database);
    let mut database = Database::open(&db.path).unwrap();
    assert!(rows(&mut database, "SELECT * FROM t WHERE id = 10").is_empty());
    assert_eq!(
        rows(&mut database, "SELECT id FROM t WHERE id = 9999"),
        vec![vec![Value::Integer(9999)]]
    );
    database
        .execute("CREATE TABLE names (name TEXT PRIMARY KEY)")
        .unwrap();
    assert_eq!(
        database
            .plan("SELECT * FROM names WHERE name = 'x'")
            .unwrap()[0]
            .access,
        AccessPath::Sequential
    );
}

#[test]
fn semantic_and_capacity_errors_do_not_change_files_or_poison_the_handle() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL); INSERT INTO t VALUES (1, 'A'), (2, 'B');").unwrap();
    let before = snapshot(&db.path);
    for source in [
        "INSERT INTO t VALUES (3, 'C'), (1, 'duplicate')".to_owned(),
        "UPDATE t SET id = 10".to_owned(),
        "UPDATE t SET name = NULL".to_owned(),
        "DELETE FROM t WHERE missing = 1".to_owned(),
        "CREATE TABLE t (id INTEGER)".to_owned(),
        format!("INSERT INTO t VALUES (3, '{}')", "x".repeat(5000)),
        format!("UPDATE t SET name = '{}'", "y".repeat(5000)),
        format!(
            "CREATE TABLE large ({})",
            (0..500)
                .map(|index| format!("column_{index} INTEGER"))
                .collect::<Vec<_>>()
                .join(",")
        ),
        "INSERT INTO t VALUES (3, 'C'); SELECT FROM t".to_owned(),
    ] {
        assert!(database.execute(&source).is_err(), "accepted {source}");
        assert_eq!(snapshot(&db.path), before);
        assert_eq!(
            rows(&mut database, "SELECT COUNT(*) FROM t"),
            vec![vec![Value::Integer(2)]]
        );
    }
    assert!(database.query("INSERT INTO t VALUES (3, 'C')").is_err());
    assert!(database.plan("DELETE FROM t").is_err());
    assert_eq!(snapshot(&db.path), before);
    assert!(
        database
            .execute("INSERT INTO t VALUES (3, 'C'); INSERT INTO t VALUES (1, 'duplicate')")
            .is_err()
    );
    drop(database);
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT COUNT(*) FROM t"),
        vec![vec![Value::Integer(3)]]
    );
}

#[test]
fn database_creation_preserves_existing_paths_and_quoted_names_use_numeric_files() {
    let db = TestDb::new();
    assert_storage_error(Database::open(&db.path), ErrorKind::NotFound);
    let mut database = Database::create(&db.path).unwrap();
    let before = snapshot(&db.path);
    assert_storage_error(Database::create(&db.path), ErrorKind::AlreadyExists);
    assert_eq!(snapshot(&db.path), before);
    database.execute("CREATE TABLE \"../odd/table\" (id INTEGER PRIMARY KEY, \"a.b\" TEXT); INSERT INTO \"../odd/table\" VALUES (1, 'safe');").unwrap();
    drop(database);
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(
            &mut database,
            "SELECT \"a.b\" FROM \"../odd/table\" WHERE id = 1"
        ),
        vec![vec![Value::Text("safe".into())]]
    );
    let names: Vec<_> = snapshot(&db.path)
        .keys()
        .map(|path| path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        vec![".lock", "catalog.db", "index-1.db", "table-1.db", "wal.log"]
    );
}

#[test]
fn reopen_rejects_malformed_catalog_rows_schemas_and_heap_index_mismatches() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL); INSERT INTO t VALUES (1, 'A'), (2, 'B')").unwrap();
    drop(database);
    let original = snapshot(&db.path);
    let mut catalog = RecordStore::open(db.path.join("catalog.db")).unwrap();
    let (id, schema) = catalog.scan().unwrap().remove(0);
    drop(catalog);
    for (field, value) in [
        (0, Value::Integer(2)),
        (1, Value::Integer(0)),
        (2, Value::Text(String::new())),
        (4, Value::Integer(99)),
        (5, Value::Integer(2)),
        (7, Value::Text("id".into())),
    ] {
        let mut catalog = RecordStore::open(db.path.join("catalog.db")).unwrap();
        let mut corrupted = schema.clone();
        corrupted.values[field] = value;
        catalog.update(id, &corrupted).unwrap();
        drop(catalog);
        assert_storage_error(Database::open(&db.path), ErrorKind::InvalidData);
        for (path, bytes) in &original {
            fs::write(path, bytes).unwrap();
        }
    }
    let mut catalog = RecordStore::open(db.path.join("catalog.db")).unwrap();
    catalog.insert(&schema).unwrap();
    drop(catalog);
    assert_storage_error(Database::open(&db.path), ErrorKind::InvalidData);
    for (path, bytes) in &original {
        fs::write(path, bytes).unwrap();
    }

    let mut heap = RecordStore::open(db.path.join("table-1.db")).unwrap();
    let (id, _) = heap.scan().unwrap().remove(0);
    heap.update(
        id,
        &Record::new(vec![
            Value::Text("wrong type".into()),
            Value::Text("A".into()),
        ]),
    )
    .unwrap();
    drop(heap);
    assert_storage_error(Database::open(&db.path), ErrorKind::InvalidData);
    for (path, bytes) in &original {
        fs::write(path, bytes).unwrap();
    }

    let mut index = BPlusTree::open(db.path.join("index-1.db")).unwrap();
    index.delete(1).unwrap();
    index
        .insert(
            1,
            RecordId {
                page_id: 99,
                slot_id: 0,
            },
        )
        .unwrap();
    drop(index);
    assert_storage_error(Database::open(&db.path), ErrorKind::InvalidData);
    for (path, bytes) in &original {
        fs::write(path, bytes).unwrap();
    }
    fs::remove_file(db.path.join("index-1.db")).unwrap();
    assert_storage_error(Database::open(&db.path), ErrorKind::NotFound);
}

#[test]
fn sync_failure_poisoning_requires_discarding_the_database_handle() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database
        .execute("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1)")
        .unwrap();
    let moved = db.root.join("moved");
    fs::rename(&db.path, &moved).unwrap();
    assert_storage_error(database.sync(), ErrorKind::NotFound);
    assert!(
        database
            .query("SELECT * FROM t")
            .unwrap_err()
            .to_string()
            .contains("discard")
    );
    assert!(database.execute("INSERT INTO t VALUES (2)").is_err());
    drop(database);
    let mut database = Database::open(&moved).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT * FROM t"),
        vec![vec![Value::Integer(1)]]
    );
}
