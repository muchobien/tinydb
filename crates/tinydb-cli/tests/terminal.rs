use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tinydb-cli"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn piped_multiline_sql_executes_crud_and_accepts_an_eof_terminated_statement() {
    let output = run(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);\n\
        INSERT INTO users VALUES\n(1, 'Alice', 25),\n(2, 'O''Brien; 🌍', NULL);\n\
        UPDATE users SET age = 30 WHERE id = 1;\n\
        SELECT name, age FROM users WHERE id = 1; DELETE FROM users WHERE id = 1;\n\
        SELECT * FROM users",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "CREATE TABLE\n2 row(s) affected\n1 row(s) affected\nname\tage\n'Alice'\t30\n1 row(s)\n1 row(s) affected\nid\tname\tage\n2\t'O''Brien; 🌍'\tNULL\n1 row(s)\n"
    );
}

#[test]
fn multiline_quotes_and_comments_keep_semicolons_inside_their_tokens() {
    let output = run("CREATE TABLE t (name TEXT);\n\
        /* hidden ;\nstill hidden */ INSERT INTO t VALUES ('first;\nsecond -- /*'), ('it''s done'); -- ignored ;\n\
        SELECT * FROM t;\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("'first;\\nsecond -- /*'\n'it''s done'\n2 row(s)")
    );
    let output = run("-- only comments ;\n/* another ; */");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn invalid_input_reports_positions_and_recovers_for_later_statements() {
    let output = run(
        "CREATE TABLE t (id INTEGER PRIMARY KEY);\nINSERT INTO t VALUES (1);\n\
        INSERT INTO t VALUES (1);\nSELECT FROM t;\nSELECT * FROM t WHERE @;\nSELECT * FROM t;\n",
    );
    assert!(!output.status.success());
    let errors = String::from_utf8(output.stderr).unwrap();
    assert!(errors.contains("PRIMARY KEY"), "{errors}");
    assert!(errors.contains("line 2, column 8"), "{errors}");
    assert!(errors.contains("unexpected character '@'"), "{errors}");
    assert!(errors.contains('^'), "{errors}");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .ends_with("id\n1\n1 row(s)\n")
    );
    let output = run("CREATE TABLE t (name TEXT);\nINSERT INTO t VALUES ('unfinished");
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unterminated")
    );
}

#[test]
fn default_sessions_start_empty_and_extra_arguments_are_rejected() {
    assert!(run("CREATE TABLE t (id INTEGER);").status.success());
    let output = run("SELECT * FROM t;");
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unknown table")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_tinydb-cli"))
        .args(["one", "two"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().contains("usage:"));
}

#[test]
fn persistent_sessions_keep_tables_indexes_and_join_results_across_processes() {
    let root = std::env::temp_dir().join(format!(
        "tinydb-terminal-persistent-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("database");
    for (source, expected, success) in [
        (
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, team INTEGER);\nCREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT);\nINSERT INTO users VALUES (1, 'Alice', 10), (2, 'Bob', 10);\nINSERT INTO teams VALUES (10, 'Blue');\nBEGIN;\nINSERT INTO users VALUES (3, 'uncommitted', 10);\n",
            "1 row(s) affected\n",
            true,
        ),
        (
            "SELECT u.name AS person, t.name AS team FROM users AS u JOIN teams AS t ON u.team = t.id ORDER BY person;\n",
            "person\tteam\n'Alice'\t'Blue'\n'Bob'\t'Blue'\n2 row(s)\n",
            true,
        ),
        (
            "BEGIN;\nUPDATE users SET id = 20 WHERE id = 2;\nDELETE FROM users WHERE id = 1;\nCOMMIT;\nSELECT id FROM users WHERE id >= 10;\n",
            "id\n20\n1 row(s)\n",
            true,
        ),
        (
            "INSERT INTO users VALUES (20, 'duplicate', 10);\n",
            "",
            false,
        ),
        (
            "SELECT COUNT(*) AS n FROM users;\nSELECT name FROM users WHERE id = 20;\n",
            "n\n1\n1 row(s)\nname\n'Bob'\n1 row(s)\n",
            true,
        ),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tinydb-cli"))
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.ends_with(expected), "{stdout}");
    }
    assert!(path.join("catalog.db").exists());
    assert!(path.join("table-1.db").exists());
    assert!(path.join("index-1.db").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn memory_terminal_transactions_commit_and_roll_back_schema_and_rows() {
    let output = run(
        "BEGIN;\nCREATE TABLE transient (id INTEGER);\nROLLBACK;\nCREATE TABLE t (id INTEGER);\nBEGIN TRANSACTION;\nINSERT INTO t VALUES (1);\nCOMMIT TRANSACTION;\nBEGIN;\nDELETE FROM t;\nROLLBACK TRANSACTION;\nSELECT * FROM t;\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "BEGIN\nCREATE TABLE\nROLLBACK\nCREATE TABLE\nBEGIN\n1 row(s) affected\nCOMMIT\nBEGIN\n1 row(s) affected\nROLLBACK\nid\n1\n1 row(s)\n"
    );
}
