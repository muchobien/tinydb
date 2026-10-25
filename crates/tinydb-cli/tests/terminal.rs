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
fn each_process_starts_with_an_empty_session_and_rejects_path_arguments() {
    assert!(run("CREATE TABLE t (id INTEGER);").status.success());
    let output = run("SELECT * FROM t;");
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unknown table")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_tinydb-cli"))
        .arg("database.db")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().contains("usage:"));
}
