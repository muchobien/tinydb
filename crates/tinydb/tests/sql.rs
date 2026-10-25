use tinydb::sql::{
    Comparison, DataType, Expr, Output, Projection, Session, Statement, TokenKind, parse, tokenize,
};
use tinydb::storage::Value;

fn execute(session: &mut Session, source: &str) -> Output {
    let statements = parse(source).unwrap();
    assert_eq!(statements.len(), 1);
    session.execute(&statements[0]).unwrap()
}

fn rows(session: &mut Session, source: &str) -> Vec<Vec<Value>> {
    let Output::Rows { rows, .. } = execute(session, source) else {
        panic!("expected query rows");
    };
    rows
}

fn reject(session: &mut Session, source: &str, message: &str) {
    let statements = parse(source).unwrap();
    assert_eq!(statements.len(), 1);
    let error = session.execute(&statements[0]).unwrap_err();
    assert!(error.0.contains(message), "{source}: {error}");
}

#[test]
fn lexer_preserves_source_spans_quoted_values_and_comments() {
    let source = "-- SELECT ignored\nSeLeCt \"odd\"\"Name\", 'O''Brien; 🌍', +42 /* hidden ; */ FROM café WHERE age != -1;";
    let tokens = tokenize(source).unwrap();
    assert_eq!(
        tokens
            .iter()
            .map(|token| token.kind.clone())
            .collect::<Vec<_>>(),
        vec![
            TokenKind::Keyword("SELECT".into()),
            TokenKind::Identifier("odd\"name".into()),
            TokenKind::Comma,
            TokenKind::String("O'Brien; 🌍".into()),
            TokenKind::Comma,
            TokenKind::Plus,
            TokenKind::Number("42".into()),
            TokenKind::Keyword("FROM".into()),
            TokenKind::Identifier("café".into()),
            TokenKind::Keyword("WHERE".into()),
            TokenKind::Identifier("age".into()),
            TokenKind::NotEqual,
            TokenKind::Minus,
            TokenKind::Number("1".into()),
            TokenKind::Semicolon,
        ]
    );
    assert_eq!(&source[tokens[0].span.clone()], "SeLeCt");
    assert_eq!(&source[tokens[1].span.clone()], "\"odd\"\"Name\"");
    assert_eq!(&source[tokens[3].span.clone()], "'O''Brien; 🌍'");
    assert_eq!(&source[tokens[8].span.clone()], "café");
    assert_eq!(
        tokenize("= != <> < <= > >=")
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>(),
        vec![
            TokenKind::Equal,
            TokenKind::NotEqual,
            TokenKind::NotEqual,
            TokenKind::Less,
            TokenKind::LessEqual,
            TokenKind::Greater,
            TokenKind::GreaterEqual,
        ]
    );
    assert!(tokenize("-- comment without newline").unwrap().is_empty());
    assert!(parse("; /* only comments */ ;; -- end").unwrap().is_empty());
}

#[test]
fn all_statement_kinds_retain_schema_values_projections_and_assignments() {
    let statements = parse("\
        CREATE TABLE Users (id INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL, age INTEGER);
        INSERT INTO users (id, name) VALUES (-9223372036854775808, 'Alice'), (+9223372036854775807, 'Bob');
        SELECT name, id FROM users WHERE age IS NOT NULL;
        UPDATE users SET age = 25, name = 'Alice García' WHERE id = -9223372036854775808;
        DELETE FROM users;
    ").unwrap();
    assert_eq!(statements.len(), 5);
    let Statement::CreateTable { name, columns } = &statements[0] else {
        panic!("expected CREATE");
    };
    assert_eq!(name, "users");
    assert_eq!(columns.len(), 3);
    assert_eq!(columns[0].data_type, DataType::Integer);
    assert!(columns[0].primary_key && columns[0].not_null);
    assert_eq!(columns[1].data_type, DataType::Text);
    assert!(columns[1].not_null);
    let Statement::Insert {
        table,
        columns,
        rows,
    } = &statements[1]
    else {
        panic!("expected INSERT");
    };
    assert_eq!(table, "users");
    assert_eq!(columns, &Some(vec!["id".into(), "name".into()]));
    assert_eq!(
        rows,
        &vec![
            vec![Value::Integer(i64::MIN), Value::Text("Alice".into())],
            vec![Value::Integer(i64::MAX), Value::Text("Bob".into())]
        ]
    );
    let Statement::Select {
        projection, filter, ..
    } = &statements[2]
    else {
        panic!("expected SELECT");
    };
    assert_eq!(
        projection,
        &Projection::Columns(vec!["name".into(), "id".into()])
    );
    assert_eq!(
        filter,
        &Some(Expr::IsNull {
            expression: Box::new(Expr::Column("age".into())),
            negated: true
        })
    );
    let Statement::Update { assignments, .. } = &statements[3] else {
        panic!("expected UPDATE");
    };
    assert_eq!(
        assignments,
        &vec![
            ("age".into(), Value::Integer(25)),
            ("name".into(), Value::Text("Alice García".into()))
        ]
    );
    assert_eq!(
        statements[4],
        Statement::Delete {
            table: "users".into(),
            filter: None
        }
    );
}

#[test]
fn boolean_precedence_and_parentheses_produce_distinct_results() {
    let mut session = Session::default();
    execute(
        &mut session,
        "CREATE TABLE users (id INTEGER, age INTEGER, name TEXT)",
    );
    execute(
        &mut session,
        "INSERT INTO users VALUES (1, 10, 'A'), (2, 20, 'B'), (3, 30, 'C'), (4, NULL, 'D')",
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT id FROM users WHERE age < 15 OR age > 25 AND name = 'C'"
        ),
        vec![vec![Value::Integer(1)], vec![Value::Integer(3)]]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT id FROM users WHERE (age < 15 OR age > 25) AND name = 'C'"
        ),
        vec![vec![Value::Integer(3)]]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT id FROM users WHERE NOT age < 20 AND age <= 30"
        ),
        vec![vec![Value::Integer(2)], vec![Value::Integer(3)]]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT id FROM users WHERE NOT (age < 20 AND name = 'A') OR age IS NULL"
        ),
        vec![
            vec![Value::Integer(2)],
            vec![Value::Integer(3)],
            vec![Value::Integer(4)]
        ]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT id FROM users WHERE age >= 20 AND age != 30 AND age <> 10"
        ),
        vec![vec![Value::Integer(2)]]
    );
    let statements =
        parse("SELECT * FROM users WHERE NOT age < 20 AND id = 2 OR age IS NULL").unwrap();
    let Statement::Select {
        filter: Some(Expr::Or(alternatives)),
        ..
    } = &statements[0]
    else {
        panic!("expected OR root");
    };
    let Expr::And(conditions) = &alternatives[0] else {
        panic!("expected AND child");
    };
    let Expr::Not(negated) = &conditions[0] else {
        panic!("expected NOT child");
    };
    assert!(matches!(
        negated.as_ref(),
        Expr::Compare {
            operator: Comparison::Less,
            ..
        }
    ));
}

#[test]
fn syntax_errors_identify_unicode_positions_eof_and_integer_overflow() {
    let source = "SELECT café\nFROM users WHERE café = @";
    let error = parse(source).unwrap_err();
    assert_eq!(error.span, source.find('@').unwrap()..source.len());
    assert_eq!((error.line, error.column), (2, 25));
    let error = parse("SELECT * FROM").unwrap_err();
    assert_eq!(error.span, 13..13);
    assert_eq!((error.line, error.column), (1, 14));
    for source in [
        "INSERT INTO users VALUES (9223372036854775808)",
        "INSERT INTO users VALUES (-9223372036854775809)",
    ] {
        let error = parse(source).unwrap_err();
        assert!(error.message.contains("i64"));
        assert!(source[error.span].starts_with(['9', '-']));
    }
    for source in [
        "CREATE TABLE users ()",
        "CREATE TABLE users (id REAL)",
        "CREATE TABLE users (id INTEGER NOT NULL NOT NULL)",
        "CREATE TABLE users (id INTEGER PRIMARY KEY PRIMARY KEY)",
        "INSERT INTO users VALUES ()",
        "INSERT INTO users VALUES (1,)",
        "INSERT INTO users VALUES (-'x')",
        "SELECT FROM users",
        "SELECT * users",
        "SELECT * FROM users WHERE id =",
        "SELECT * FROM users WHERE id = 1 = 2",
        "UPDATE users SET id = id + 1",
        "DELETE users",
        "SELECT * FROM users SELECT * FROM users",
        "SELECT * FROM users ORDER BY id",
        "SELECT * FROM users WHERE id ! 2",
        "SELECT * FROM users WHERE id = 1.5",
        "SELECT * FROM users WHERE id = ?",
        "SELECT \"\" FROM users",
        "SELECT * FROM users /* unfinished",
        "INSERT INTO users VALUES ('unfinished)",
    ] {
        assert!(parse(source).is_err(), "accepted {source}");
    }
}

#[test]
fn expression_nesting_is_bounded_and_flat_boolean_lists_remain_safe() {
    assert!(
        parse(&format!(
            "SELECT * FROM t WHERE {}1{}",
            "(".repeat(63),
            ")".repeat(63)
        ))
        .is_ok()
    );
    let error = parse(&format!(
        "SELECT * FROM t WHERE {}1{}",
        "(".repeat(1000),
        ")".repeat(1000)
    ))
    .unwrap_err();
    assert!(error.message.contains("nesting"));
    assert!(
        parse(&format!("SELECT * FROM t WHERE {}1", "NOT ".repeat(1000)))
            .unwrap_err()
            .message
            .contains("nesting")
    );
    let mut session = Session::default();
    execute(&mut session, "CREATE TABLE t (id INTEGER)");
    execute(&mut session, "INSERT INTO t VALUES (1)");
    let source = format!(
        "SELECT * FROM t WHERE {}",
        vec!["id = 1"; 5000].join(" AND ")
    );
    assert_eq!(rows(&mut session, &source), vec![vec![Value::Integer(1)]]);
}

#[test]
fn terminal_language_supports_crud_quoted_names_and_omitted_nullable_columns() {
    let mut session = Session::default();
    assert_eq!(
        execute(
            &mut session,
            "CREATE TABLE \"Select\" (\"User ID\" INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)"
        ),
        Output::Created
    );
    assert_eq!(
        execute(
            &mut session,
            "INSERT INTO \"SELECT\" (name, \"user id\") VALUES ('O''Brien; 🌍', 1), ('Bob', 2)"
        ),
        Output::Affected(2)
    );
    assert_eq!(
        rows(&mut session, "SELECT age FROM \"select\""),
        vec![vec![Value::Null], vec![Value::Null]]
    );
    assert_eq!(
        execute(
            &mut session,
            "UPDATE \"select\" SET name = 'Alice', age = 25 WHERE \"User ID\" = 1"
        ),
        Output::Affected(1)
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT name, age FROM \"select\" WHERE age IS NOT NULL"
        ),
        vec![vec![Value::Text("Alice".into()), Value::Integer(25)]]
    );
    assert_eq!(
        execute(&mut session, "DELETE FROM \"select\" WHERE name = 'Bob'"),
        Output::Affected(1)
    );
    assert_eq!(
        rows(&mut session, "SELECT * FROM \"select\""),
        vec![vec![
            Value::Integer(1),
            Value::Text("Alice".into()),
            Value::Integer(25)
        ]]
    );
    assert_eq!(
        execute(&mut session, "UPDATE \"select\" SET age = NULL"),
        Output::Affected(1)
    );
    assert_eq!(
        execute(&mut session, "DELETE FROM \"select\""),
        Output::Affected(1)
    );
    assert!(rows(&mut session, "SELECT * FROM \"select\"").is_empty());
}

#[test]
fn failed_multirow_inserts_and_updates_leave_existing_rows_unchanged() {
    let mut session = Session::default();
    execute(
        &mut session,
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)",
    );
    execute(
        &mut session,
        "INSERT INTO users VALUES (1, 'Alice', 25), (2, 'Bob', NULL)",
    );
    let before = rows(&mut session, "SELECT * FROM users");
    for (source, message) in [
        (
            "INSERT INTO users VALUES (3, 'C', 30), (1, 'D', 40)",
            "PRIMARY KEY",
        ),
        (
            "INSERT INTO users VALUES (3, 'C', 30), (3, 'D', 40)",
            "PRIMARY KEY",
        ),
        (
            "INSERT INTO users VALUES (3, 'C', 30), (4, 'D', 'wrong')",
            "type",
        ),
        (
            "INSERT INTO users VALUES (3, 'C', 30), (4, 'D')",
            "expected 3 values",
        ),
        ("INSERT INTO users (id) VALUES (3)", "NULL"),
        ("INSERT INTO users (name) VALUES ('C')", "NULL"),
        ("INSERT INTO users (id, id) VALUES (3, 4)", "unique"),
        ("UPDATE users SET id = 10", "PRIMARY KEY"),
        ("UPDATE users SET name = NULL", "NULL"),
        ("UPDATE users SET age = 'bad' WHERE id = 1", "type"),
        ("UPDATE users SET age = 1, age = 2", "duplicate"),
        (
            "UPDATE users SET age = 1 WHERE missing = 2",
            "unknown column",
        ),
        ("DELETE FROM users WHERE id = '1'", "matching"),
    ] {
        reject(&mut session, source, message);
        assert_eq!(
            rows(&mut session, "SELECT * FROM users"),
            before,
            "{source}"
        );
    }
    reject(
        &mut session,
        "CREATE TABLE users (id INTEGER)",
        "already exists",
    );
    reject(
        &mut session,
        "CREATE TABLE duplicate (id INTEGER, ID TEXT)",
        "unique",
    );
    reject(
        &mut session,
        "CREATE TABLE composite (id INTEGER PRIMARY KEY, name TEXT PRIMARY KEY)",
        "one PRIMARY KEY",
    );
    execute(&mut session, "CREATE TABLE names (name TEXT PRIMARY KEY)");
    execute(&mut session, "INSERT INTO names VALUES ('Alice')");
    reject(
        &mut session,
        "INSERT INTO names VALUES ('Alice')",
        "PRIMARY KEY",
    );
}

#[test]
fn null_comparisons_and_boolean_operators_follow_three_valued_logic() {
    let mut session = Session::default();
    execute(&mut session, "CREATE TABLE t (id INTEGER)");
    execute(&mut session, "INSERT INTO t VALUES (1)");
    for (predicate, matches) in [
        ("NULL = NULL", false),
        ("NULL != NULL", false),
        ("NOT NULL", false),
        ("NULL IS NULL", true),
        ("NULL IS NOT NULL", false),
        ("NULL AND 0", false),
        ("NOT (NULL AND 0)", true),
        ("NULL AND 1", false),
        ("NOT (NULL AND 1)", false),
        ("NULL OR 1", true),
        ("NULL OR 0", false),
        ("NOT (NULL OR 0)", false),
        ("NULL OR 0 OR 1", true),
        ("NULL AND 1 AND 0", false),
        ("1 OR NULL", true),
        ("0 AND NULL", false),
        ("-2", true),
        ("0", false),
    ] {
        assert_eq!(
            rows(&mut session, &format!("SELECT * FROM t WHERE {predicate}")).len(),
            usize::from(matches),
            "{predicate}"
        );
    }
}

#[test]
fn schema_and_predicate_errors_are_reported_even_for_empty_tables() {
    let mut session = Session::default();
    execute(&mut session, "CREATE TABLE t (id INTEGER, name TEXT)");
    for (source, message) in [
        ("SELECT * FROM missing", "unknown table"),
        ("SELECT missing FROM t", "unknown column"),
        ("SELECT * FROM t WHERE missing = 1", "unknown column"),
        ("DELETE FROM t WHERE name = 1", "matching"),
        ("SELECT * FROM t WHERE name", "boolean"),
        ("SELECT * FROM t WHERE NOT name", "boolean"),
        ("SELECT * FROM t WHERE id AND name", "boolean"),
        ("SELECT * FROM t WHERE id OR name", "boolean"),
        ("SELECT * FROM t WHERE (name = 1) IS NULL", "matching"),
        ("UPDATE t SET name = 1", "type"),
        ("UPDATE t SET missing = NULL", "unknown column"),
        ("INSERT INTO t (missing) VALUES (1)", "unknown column"),
    ] {
        reject(&mut session, source, message);
    }
}
