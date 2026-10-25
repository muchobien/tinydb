use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tinydb::sql::{Output, Session, parse};
use tinydb::storage::Value;
use tinydb::transaction::Journal;
use tinydb::{Database, DatabaseError};

struct TestDb {
    root: PathBuf,
    path: PathBuf,
    staged: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "tinydb-transaction-test-{}-{}-{}",
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
            staged: root.join("staged"),
            root,
        }
    }

    fn prepare(&self) -> Vec<u8> {
        let mut database = Database::create(&self.path).unwrap();
        database.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'old')").unwrap();
        drop(database);
        let mut staged = Database::create(&self.staged).unwrap();
        staged.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'new'), (2, 'added'); CREATE TABLE extra (id INTEGER PRIMARY KEY); INSERT INTO extra VALUES (9)").unwrap();
        drop(staged);
        let mut journal = Journal::open(&self.path).unwrap();
        journal.write_transaction(&self.staged).unwrap();
        fs::read(self.path.join("wal.log")).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn rows(database: &mut Database, source: &str) -> Vec<Vec<Value>> {
    let Output::Rows { rows, .. } = database.query(source).unwrap() else {
        panic!("expected rows");
    };
    rows
}

fn home_files(path: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            if name.ends_with(".db") {
                Some((name, fs::read(path).unwrap()))
            } else {
                None
            }
        })
        .collect()
}

fn run_child(db: &TestDb, mode: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "transaction_child_process", "--nocapture"])
        .env("TINYDB_TRANSACTION_CHILD_PATH", &db.path)
        .env("TINYDB_TRANSACTION_CHILD_STAGED", &db.staged)
        .env("TINYDB_TRANSACTION_CHILD_MODE", mode)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{mode}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn explicit_transactions_share_memory_semantics_and_keep_home_files_unchanged_until_commit() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    let mut session = Session::default();
    for statement in parse(
        "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'old');",
    )
    .unwrap()
    {
        assert_eq!(
            database.execute_statement(&statement).unwrap(),
            session.execute(&statement).unwrap()
        );
    }
    let before = home_files(&db.path);
    for statement in parse("BEGIN TRANSACTION; UPDATE t SET name = 'updated' WHERE id = 1; INSERT INTO t VALUES (2, 'second'); CREATE TABLE transient (id INTEGER PRIMARY KEY); INSERT INTO transient VALUES (8); SELECT * FROM t ORDER BY id; SELECT * FROM transient;").unwrap() {
        assert_eq!(database.execute_statement(&statement).unwrap(), session.execute(&statement).unwrap());
        assert_eq!(home_files(&db.path), before);
    }
    database.sync().unwrap();
    assert_eq!(home_files(&db.path), before);
    assert_eq!(
        database.execute("ROLLBACK TRANSACTION").unwrap(),
        vec![Output::RolledBack]
    );
    session.execute(&parse("ROLLBACK").unwrap()[0]).unwrap();
    assert_eq!(home_files(&db.path), before);
    assert!(!db.path.join(".transaction").exists());
    assert!(database.query("SELECT * FROM transient").is_err());
    assert_eq!(
        rows(&mut database, "SELECT * FROM t"),
        vec![vec![Value::Integer(1), Value::Text("old".into())]]
    );
    for statement in parse("BEGIN; UPDATE t SET id = 3; INSERT INTO t VALUES (2, 'second'); CREATE TABLE durable (id INTEGER PRIMARY KEY); INSERT INTO durable VALUES (9); COMMIT;").unwrap() {
        assert_eq!(database.execute_statement(&statement).unwrap(), session.execute(&statement).unwrap());
    }
    drop(database);
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT id FROM t ORDER BY id"),
        vec![vec![Value::Integer(2)], vec![Value::Integer(3)]]
    );
    assert_eq!(
        rows(&mut database, "SELECT * FROM durable"),
        vec![vec![Value::Integer(9)]]
    );
    assert_eq!(fs::metadata(db.path.join("wal.log")).unwrap().len(), 0);
}

#[test]
fn transaction_errors_preserve_the_active_transaction_and_drop_rolls_it_back() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database
        .execute(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'old')",
        )
        .unwrap();
    assert!(database.execute("COMMIT").is_err());
    assert!(database.execute("ROLLBACK").is_err());
    let before = home_files(&db.path);
    database
        .execute("BEGIN; INSERT INTO t VALUES (2, 'pending')")
        .unwrap();
    for source in [
        "BEGIN",
        "INSERT INTO t VALUES (2, 'duplicate')",
        "UPDATE t SET id = 3",
        "CREATE TABLE t (id INTEGER)",
    ] {
        assert!(database.execute(source).is_err());
        assert_eq!(
            rows(&mut database, "SELECT COUNT(*) FROM t"),
            vec![vec![Value::Integer(2)]]
        );
    }
    database
        .execute("INSERT INTO t VALUES (3, 'valid'); COMMIT")
        .unwrap();
    assert_ne!(home_files(&db.path), before);
    let committed = home_files(&db.path);
    database
        .execute("BEGIN; DELETE FROM t; CREATE TABLE dropped (id INTEGER)")
        .unwrap();
    drop(database);
    assert_eq!(home_files(&db.path), committed);
    assert!(!db.path.join(".transaction").exists());
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT COUNT(*) FROM t"),
        vec![vec![Value::Integer(3)]]
    );
    assert!(database.query("SELECT * FROM dropped").is_err());
    for source in [
        "BEGIN IMMEDIATE",
        "BEGIN TRANSACTION extra",
        "COMMIT extra",
        "ROLLBACK TO x",
    ] {
        assert!(parse(source).is_err());
    }
}

#[test]
fn committed_journal_recovery_is_idempotent_and_runs_before_catalog_validation() {
    let db = TestDb::new();
    let bytes = db.prepare();
    assert_eq!(&bytes[..8], b"TDBWAL1\0");
    assert_eq!(&bytes[bytes.len() - 24..bytes.len() - 16], b"TDBCMT1\0");
    assert_eq!(u64::from_le_bytes(bytes[8..16].try_into().unwrap()), 5);
    let before = home_files(&db.path);
    assert_ne!(before, home_files(&db.staged));
    let mut journal = Journal::open(&db.path).unwrap();
    assert!(journal.write_transaction(&db.staged).is_err());
    assert!(journal.recover().unwrap());
    assert_eq!(home_files(&db.path), home_files(&db.staged));
    assert!(!journal.recover().unwrap());
    fs::write(db.path.join("wal.log"), &bytes).unwrap();
    assert!(journal.recover().unwrap());
    assert_eq!(home_files(&db.path), home_files(&db.staged));
    drop(journal);
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT name FROM t ORDER BY id"),
        vec![
            vec![Value::Text("new".into())],
            vec![Value::Text("added".into())]
        ]
    );
    assert_eq!(
        rows(&mut database, "SELECT * FROM extra"),
        vec![vec![Value::Integer(9)]]
    );
}

#[test]
fn torn_wal_prefixes_and_commit_records_are_discarded_without_touching_home_files() {
    let db = TestDb::new();
    let bytes = db.prepare();
    let before = home_files(&db.path);
    let mut cuts: Vec<_> = (0..39).collect();
    cuts.extend([100, bytes.len() / 2]);
    cuts.extend(bytes.len() - 24..bytes.len());
    for cut in cuts {
        fs::write(db.path.join("wal.log"), &bytes[..cut]).unwrap();
        let mut journal = Journal::open(&db.path).unwrap();
        assert!(!journal.recover().unwrap(), "committed prefix length {cut}");
        assert_eq!(home_files(&db.path), before, "prefix {cut}");
        assert_eq!(fs::metadata(db.path.join("wal.log")).unwrap().len(), 0);
    }
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT name FROM t"),
        vec![vec![Value::Text("old".into())]]
    );
}

#[test]
fn corrupt_wal_headers_images_and_commit_records_fail_before_home_writes() {
    let db = TestDb::new();
    let bytes = db.prepare();
    let before = home_files(&db.path);
    for offset in [
        0,
        8,
        16,
        20,
        22,
        30,
        34,
        38,
        100,
        bytes.len() - 24,
        bytes.len() - 16,
        bytes.len() - 8,
        bytes.len() - 4,
    ] {
        let mut corrupted = bytes.clone();
        corrupted[offset] ^= 1;
        fs::write(db.path.join("wal.log"), &corrupted).unwrap();
        let mut journal = Journal::open(&db.path).unwrap();
        assert_eq!(
            journal.recover().unwrap_err().kind(),
            ErrorKind::InvalidData,
            "offset {offset}"
        );
        assert_eq!(home_files(&db.path), before);
        assert_eq!(fs::read(db.path.join("wal.log")).unwrap(), corrupted);
    }
    let mut trailing = bytes;
    trailing.push(0);
    fs::write(db.path.join("wal.log"), &trailing).unwrap();
    let mut journal = Journal::open(&db.path).unwrap();
    assert_eq!(
        journal.recover().unwrap_err().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(home_files(&db.path), before);
}

#[test]
fn exclusive_locking_blocks_other_handles_and_processes_until_drop() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database
        .execute("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1)")
        .unwrap();
    assert!(
        matches!(Database::open(&db.path), Err(DatabaseError::Storage(error)) if error.kind() == ErrorKind::WouldBlock)
    );
    run_child(&db, "locked");
    database.execute("BEGIN; INSERT INTO t VALUES (2)").unwrap();
    run_child(&db, "locked");
    drop(database);
    run_child(&db, "available");
}

#[test]
fn process_exit_recovery_discards_uncommitted_changes_and_replays_durable_commits() {
    for mode in [
        "uncommitted",
        "normal_commit",
        "committed_wal",
        "partial_commit",
        "partial_checkpoint",
    ] {
        let db = TestDb::new();
        let mut database = Database::create(&db.path).unwrap();
        database.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'old')").unwrap();
        drop(database);
        run_child(&db, mode);
        if mode == "partial_checkpoint" {
            fs::remove_dir(db.path.join("index-2.db")).unwrap();
        }
        let mut database = Database::open(&db.path).unwrap();
        let committed = matches!(
            mode,
            "normal_commit" | "committed_wal" | "partial_checkpoint"
        );
        assert_eq!(
            rows(&mut database, "SELECT name FROM t ORDER BY id"),
            if committed {
                vec![
                    vec![Value::Text("new".into())],
                    vec![Value::Text("added".into())],
                ]
            } else {
                vec![vec![Value::Text("old".into())]]
            },
            "{mode}"
        );
        assert_eq!(
            database.query("SELECT * FROM extra").is_ok(),
            committed,
            "{mode}"
        );
        assert!(!db.path.join(".transaction").exists());
        assert_eq!(fs::metadata(db.path.join("wal.log")).unwrap().len(), 0);
    }
}

#[test]
fn a_checkpoint_error_after_the_commit_record_is_recovered_on_reopen() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'old'); BEGIN; UPDATE t SET name = 'new'; CREATE TABLE extra (id INTEGER PRIMARY KEY); INSERT INTO extra VALUES (9)").unwrap();
    fs::create_dir(db.path.join(".checkpoint.tmp")).unwrap();
    assert!(matches!(
        database.execute("COMMIT"),
        Err(DatabaseError::Storage(_))
    ));
    assert!(
        database
            .query("SELECT * FROM t")
            .unwrap_err()
            .to_string()
            .contains("discard")
    );
    assert!(fs::metadata(db.path.join("wal.log")).unwrap().len() > 0);
    drop(database);
    fs::remove_dir(db.path.join(".checkpoint.tmp")).unwrap();
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT name FROM t"),
        vec![vec![Value::Text("new".into())]]
    );
    assert_eq!(
        rows(&mut database, "SELECT * FROM extra"),
        vec![vec![Value::Integer(9)]]
    );
}

#[test]
fn killing_an_active_writer_releases_its_lock_and_discards_staging() {
    let db = TestDb::new();
    let mut database = Database::create(&db.path).unwrap();
    database
        .execute(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'old')",
        )
        .unwrap();
    drop(database);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "transaction_child_process", "--nocapture"])
        .env("TINYDB_TRANSACTION_CHILD_PATH", &db.path)
        .env("TINYDB_TRANSACTION_CHILD_MODE", "wait_uncommitted")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let reader = BufReader::new(child.stdout.take().unwrap());
    assert!(
        reader
            .lines()
            .any(|line| line.unwrap().contains("transaction-ready"))
    );
    assert!(db.path.join(".transaction").exists());
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let mut database = Database::open(&db.path).unwrap();
    assert_eq!(
        rows(&mut database, "SELECT name FROM t"),
        vec![vec![Value::Text("old".into())]]
    );
    assert!(!db.path.join(".transaction").exists());
}

#[cfg(unix)]
#[test]
fn control_files_are_private_and_recovery_rejects_symlink_targets() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let db = TestDb::new();
    db.prepare();
    for name in [".lock", "wal.log"] {
        assert_eq!(
            fs::metadata(db.path.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let outside = db.root.join("outside");
    fs::write(&outside, "preserve this").unwrap();
    symlink(&outside, db.path.join(".checkpoint.tmp")).unwrap();
    let before = home_files(&db.path);
    let mut journal = Journal::open(&db.path).unwrap();
    assert_eq!(
        journal.recover().unwrap_err().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "preserve this");
    assert_eq!(home_files(&db.path), before);
    drop(journal);
    fs::remove_file(db.path.join(".checkpoint.tmp")).unwrap();
    let mut database = Database::open(&db.path).unwrap();
    database.execute("BEGIN").unwrap();
    assert_eq!(
        fs::metadata(db.path.join(".transaction"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    database.execute("ROLLBACK").unwrap();
    drop(database);
    fs::remove_file(db.path.join(".lock")).unwrap();
    symlink(&outside, db.path.join(".lock")).unwrap();
    assert!(
        matches!(Database::open(&db.path), Err(DatabaseError::Storage(error)) if error.kind() == ErrorKind::InvalidData)
    );
    assert_eq!(fs::read_to_string(outside).unwrap(), "preserve this");
}

#[test]
fn transaction_child_process() {
    let Some(path) = std::env::var_os("TINYDB_TRANSACTION_CHILD_PATH").map(PathBuf::from) else {
        return;
    };
    let mode = std::env::var("TINYDB_TRANSACTION_CHILD_MODE").unwrap();
    match mode.as_str() {
        "locked" => {
            assert!(
                matches!(Database::open(&path), Err(DatabaseError::Storage(error)) if error.kind() == ErrorKind::WouldBlock)
            );
        }
        "available" => {
            let mut database = Database::open(&path).unwrap();
            assert_eq!(
                rows(&mut database, "SELECT COUNT(*) FROM t"),
                vec![vec![Value::Integer(1)]]
            );
        }
        "uncommitted" | "normal_commit" | "wait_uncommitted" => {
            let mut database = Database::open(&path).unwrap();
            database.execute("BEGIN; UPDATE t SET name = 'new'; INSERT INTO t VALUES (2, 'added'); CREATE TABLE extra (id INTEGER PRIMARY KEY); INSERT INTO extra VALUES (9)").unwrap();
            if mode == "normal_commit" {
                database.execute("COMMIT").unwrap();
            }
            if mode == "wait_uncommitted" {
                println!("transaction-ready");
                std::io::stdout().flush().unwrap();
                std::io::stdin().read_line(&mut String::new()).unwrap();
            }
            std::process::exit(0);
        }
        _ => {
            let staged =
                PathBuf::from(std::env::var_os("TINYDB_TRANSACTION_CHILD_STAGED").unwrap());
            let mut database = Database::create(&staged).unwrap();
            database.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'new'), (2, 'added'); CREATE TABLE extra (id INTEGER PRIMARY KEY); INSERT INTO extra VALUES (9)").unwrap();
            drop(database);
            let mut journal = Journal::open(&path).unwrap();
            journal.write_transaction(&staged).unwrap();
            if mode == "partial_commit" {
                let wal = File::options()
                    .write(true)
                    .open(path.join("wal.log"))
                    .unwrap();
                wal.set_len(wal.metadata().unwrap().len() - 1).unwrap();
                wal.sync_all().unwrap();
            } else if mode == "partial_checkpoint" {
                fs::create_dir(path.join("index-2.db")).unwrap();
                assert!(journal.recover().is_err());
            }
            std::process::exit(0);
        }
    }
}
