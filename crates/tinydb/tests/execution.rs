use tinydb::sql::{Output, Session, parse};
use tinydb::storage::Value;

fn execute(session: &mut Session, source: &str) -> Output {
    let statements = parse(source).unwrap();
    assert_eq!(statements.len(), 1);
    session.execute(&statements[0]).unwrap()
}

fn rows(session: &mut Session, source: &str) -> Vec<Vec<Value>> {
    let Output::Rows { rows, .. } = execute(session, source) else {
        panic!("expected rows");
    };
    rows
}

fn reject(session: &mut Session, source: &str, message: &str) {
    let statement = parse(source).unwrap().remove(0);
    let error = session.execute(&statement).unwrap_err();
    assert!(error.0.contains(message), "{source}: {error}");
}

#[test]
fn inner_joins_bind_qualified_columns_aliases_and_null_predicates() {
    let mut session = Session::default();
    for statement in parse("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, team INTEGER, manager INTEGER);
        CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE labels (team INTEGER, title TEXT);
        INSERT INTO users VALUES (1, 'Alice', 10, NULL), (2, 'Bob', 10, 1), (3, 'Charlie', NULL, 1), (4, 'Dana', 20, 2);
        INSERT INTO teams VALUES (10, 'Blue'), (20, 'Green');
        INSERT INTO labels VALUES (10, 'X'), (20, 'Y');").unwrap() { session.execute(&statement).unwrap(); }
    assert_eq!(
        rows(
            &mut session,
            "SELECT u.name AS person, t.name AS team FROM users AS u INNER JOIN teams AS t ON u.team = t.id WHERE u.id >= 2 ORDER BY person DESC"
        ),
        vec![
            vec![Value::Text("Dana".into()), Value::Text("Green".into())],
            vec![Value::Text("Bob".into()), Value::Text("Blue".into())]
        ]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT u.name, l.title FROM users AS u JOIN teams AS t ON u.team = t.id JOIN labels AS l ON t.id = l.team ORDER BY u.id"
        ),
        vec![
            vec![Value::Text("Alice".into()), Value::Text("X".into())],
            vec![Value::Text("Bob".into()), Value::Text("X".into())],
            vec![Value::Text("Dana".into()), Value::Text("Y".into())]
        ]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT u.name, m.name FROM users AS u JOIN users AS m ON u.manager = m.id ORDER BY u.id"
        ),
        vec![
            vec![Value::Text("Bob".into()), Value::Text("Alice".into())],
            vec![Value::Text("Charlie".into()), Value::Text("Alice".into())],
            vec![Value::Text("Dana".into()), Value::Text("Bob".into())]
        ]
    );
    reject(
        &mut session,
        "SELECT id FROM users JOIN teams ON users.team = teams.id",
        "ambiguous",
    );
    reject(
        &mut session,
        "SELECT users.name FROM users AS u",
        "unknown column",
    );
    reject(
        &mut session,
        "SELECT * FROM users JOIN users ON users.id = users.id",
        "duplicate table qualifier",
    );
    reject(
        &mut session,
        "SELECT * FROM users JOIN teams ON labels.team = teams.id JOIN labels ON labels.team = teams.id",
        "unknown column",
    );
}

#[test]
fn grouping_aggregates_nulls_and_empty_inputs_follow_sql_semantics() {
    let mut session = Session::default();
    execute(
        &mut session,
        "CREATE TABLE sales (team TEXT, amount INTEGER, label TEXT)",
    );
    execute(
        &mut session,
        "INSERT INTO sales VALUES ('Blue', 10, 'z'), ('Blue', 20, 'a'), ('Blue', NULL, NULL), ('Green', 5, 'g'), (NULL, NULL, NULL)",
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT team, COUNT(*) AS n, COUNT(amount), SUM(amount), MIN(label), MAX(amount) FROM sales GROUP BY team ORDER BY n DESC, team ASC"
        ),
        vec![
            vec![
                Value::Text("Blue".into()),
                Value::Integer(3),
                Value::Integer(2),
                Value::Integer(30),
                Value::Text("a".into()),
                Value::Integer(20)
            ],
            vec![
                Value::Null,
                Value::Integer(1),
                Value::Integer(0),
                Value::Null,
                Value::Null,
                Value::Null
            ],
            vec![
                Value::Text("Green".into()),
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(5),
                Value::Text("g".into()),
                Value::Integer(5)
            ],
        ]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT COUNT(*), COUNT(amount), SUM(amount), MIN(label), MAX(label) FROM sales WHERE 0"
        ),
        vec![vec![
            Value::Integer(0),
            Value::Integer(0),
            Value::Null,
            Value::Null,
            Value::Null
        ]]
    );
    assert!(
        rows(
            &mut session,
            "SELECT team, COUNT(*) FROM sales WHERE 0 GROUP BY team"
        )
        .is_empty()
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT team FROM sales GROUP BY team ORDER BY team"
        ),
        vec![
            vec![Value::Null],
            vec![Value::Text("Blue".into())],
            vec![Value::Text("Green".into())]
        ]
    );
    reject(&mut session, "SELECT team, COUNT(*) FROM sales", "GROUP BY");
    reject(
        &mut session,
        "SELECT * FROM sales GROUP BY team",
        "GROUP BY",
    );
    reject(&mut session, "SELECT SUM(label) FROM sales", "INTEGER");
    reject(
        &mut session,
        "SELECT COUNT(*) FROM sales ORDER BY amount",
        "GROUP BY",
    );
    reject(
        &mut session,
        "SELECT COUNT(*) FROM sales GROUP BY missing",
        "unknown column",
    );
    execute(&mut session, "CREATE TABLE limits (amount INTEGER)");
    execute(
        &mut session,
        "INSERT INTO limits VALUES (9223372036854775807), (1)",
    );
    reject(&mut session, "SELECT SUM(amount) FROM limits", "overflow");
    assert_eq!(
        rows(&mut session, "SELECT COUNT(*) FROM limits"),
        vec![vec![Value::Integer(2)]]
    );
}

#[test]
fn ordering_supports_hidden_input_columns_aliases_and_quoted_dots() {
    let mut session = Session::default();
    execute(
        &mut session,
        "CREATE TABLE t (id INTEGER, name TEXT, \"a.b\" INTEGER)",
    );
    execute(
        &mut session,
        "INSERT INTO t VALUES (2, 'B', 9), (1, NULL, 8), (3, 'A', 7), (4, 'A', 6)",
    );
    assert_eq!(
        rows(&mut session, "SELECT name FROM t ORDER BY id DESC"),
        vec![
            vec![Value::Text("A".into())],
            vec![Value::Text("A".into())],
            vec![Value::Text("B".into())],
            vec![Value::Null]
        ]
    );
    assert_eq!(
        rows(
            &mut session,
            "SELECT id AS row_id FROM t ORDER BY name ASC, row_id DESC"
        ),
        vec![
            vec![Value::Integer(1)],
            vec![Value::Integer(4)],
            vec![Value::Integer(3)],
            vec![Value::Integer(2)]
        ]
    );
    assert_eq!(
        rows(&mut session, "SELECT \"a.b\" FROM t ORDER BY id"),
        vec![
            vec![Value::Integer(8)],
            vec![Value::Integer(9)],
            vec![Value::Integer(7)],
            vec![Value::Integer(6)]
        ]
    );
    assert_eq!(
        rows(&mut session, "SELECT id FROM t ORDER BY name DESC, id"),
        vec![
            vec![Value::Integer(2)],
            vec![Value::Integer(3)],
            vec![Value::Integer(4)],
            vec![Value::Integer(1)]
        ]
    );
    assert_eq!(
        rows(&mut session, "SELECT COUNT(*) FROM t ORDER BY \"count(*)\""),
        vec![vec![Value::Integer(4)]]
    );
    reject(
        &mut session,
        "SELECT id AS x, name AS x FROM t ORDER BY x",
        "ambiguous output",
    );
    reject(&mut session, "SELECT a.b FROM t", "unknown column");
    execute(&mut session, "CREATE TABLE empty (id INTEGER)");
    reject(
        &mut session,
        "SELECT id FROM empty ORDER BY missing",
        "unknown column",
    );
    reject(
        &mut session,
        "SELECT id FROM empty JOIN t ON empty.id = t.name",
        "matching",
    );
}

#[test]
fn malformed_join_grouping_and_ordering_syntax_is_rejected() {
    for source in [
        "SELECT * FROM t INNER t ON t.id = 1",
        "SELECT * FROM t JOIN u",
        "SELECT * FROM t JOIN u ON",
        "SELECT * FROM t AS",
        "SELECT * FROM t alias",
        "SELECT t. FROM t",
        "SELECT COUNT() FROM t",
        "SELECT SUM(*) FROM t",
        "SELECT AVG(id) FROM t",
        "SELECT * FROM t LEFT JOIN u ON t.id = u.id",
        "SELECT * FROM t GROUP id",
        "SELECT * FROM t GROUP BY",
        "SELECT * FROM t ORDER BY id SIDEWAYS",
        "SELECT * FROM t ORDER BY id,",
        "SELECT COUNT(*) FROM t GROUP BY id HAVING id = 1",
    ] {
        assert!(parse(source).is_err(), "accepted {source}");
    }
}
