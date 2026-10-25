# SQL language

Milestone 4 turns SQL text into tokens and an AST, then executes the supported
statements in an in-memory `sql::Session`. The terminal uses the same parser and
session API. Tables disappear when the process exits. The pager, heap, and
index implementations remain separate; a persistent schema catalog and
relational execution over stored tables belong to Milestone 5.

## Grammar

```text
script       = {";"} [statement {";" {";"} statement} {";"}] ;
statement    = create | insert | select | update | delete ;
create       = CREATE TABLE name "(" column {"," column} ")" ;
column       = name (INTEGER | TEXT) {NOT NULL | PRIMARY KEY} ;
insert       = INSERT INTO name ["(" names ")"] VALUES row {"," row} ;
row          = "(" literal {"," literal} ")" ;
select       = SELECT ("*" | names) FROM name [WHERE expression] ;
update       = UPDATE name SET assignment {"," assignment} [WHERE expression] ;
assignment   = name "=" literal ;
delete       = DELETE FROM name [WHERE expression] ;
names        = name {"," name} ;
expression   = conjunction {OR conjunction} ;
conjunction  = negation {AND negation} ;
negation     = NOT negation | predicate ;
predicate    = primary [comparison primary | IS [NOT] NULL] ;
primary      = name | literal | "(" expression ")" ;
comparison   = "=" | "!=" | "<>" | "<" | "<=" | ">" | ">=" ;
literal      = NULL | ["+" | "-"] digits | string ;
```

Separate statements with semicolons; the final statement may omit its
semicolon. Empty statements and comment-only input are accepted. Column lists,
row lists, and assignments require at least one item; trailing commas are
errors. Repeated column constraints are syntax errors.

Keywords ignore ASCII case. Unquoted identifiers begin with a letter or `_`,
followed by letters, digits, or `_`; Unicode letters are accepted. Double quotes
allow names containing spaces or reserved words. Double a quote inside a quoted
identifier. All identifiers normalize ASCII letters to lowercase, including
quoted names; Unicode case folding is outside this subset.

Single-quoted strings preserve Unicode and newlines. Escape a single quote by
doubling it (`'O''Brien'`). Backslashes remain literal characters. Integer
literals cover the complete signed `i64` range, including `-9223372036854775808`.
Decimals, parameters, arithmetic, and other literal types are rejected.

The lexer skips whitespace, `--` line comments, and non-nested `/* ... */`
comments. Quoted text can contain semicolons and comment markers. Tokens retain
UTF-8 byte ranges into the source. `SqlError` provides a message, byte range,
and one-based line and character column; EOF errors have an empty range.

## Parsing and evaluation

The handwritten recursive-descent parser follows the grammar's precedence:
parentheses and comparisons, then `NOT`, then `AND`, then `OR`. Comparison
chains require explicit parentheses. Recursive expression nesting is bounded
to 64 parser levels. Flat `AND`/`OR` lists use vectors in the AST, avoiding deep
recursion for long lists of conditions.

Each table has typed columns and optional column-level `NOT NULL` or
`PRIMARY KEY` constraints. A table supports one primary key column, with either
INTEGER or TEXT values. Primary keys must be unique and non-null; there is no
implicit row ID or generated key. INSERT column lists may reorder columns;
omitted columns receive NULL and must satisfy their constraints.

The session validates column references and predicate types before visiting
rows, including empty tables. Comparisons require matching INTEGER or TEXT
types; values are not coerced. Text comparisons use Rust's case-sensitive
string ordering. Any comparison with NULL produces unknown. `IS NULL` and
`IS NOT NULL` return true or false. Integer zero is false and any other integer
is true; text values cannot serve as boolean conditions. `NOT`, `AND`, and `OR`
use three-valued logic. WHERE selects only true results; false and unknown are
excluded. All logical operands are validated and evaluated, with no
short-circuit guarantee.

INSERT and UPDATE prepare and validate candidate rows before replacing table
state. A failed statement leaves the session unchanged, including a failure
late in a multirow insert or an update that produces duplicate primary keys.
DELETE validates its predicate before removing rows. Earlier successful
statements remain applied when a later statement fails; there are no script
transactions. This small session clones rows for mutation validation and scans
tables; storage-backed execution and query planning come next.

## Library API

```rust
use tinydb::sql::{Output, Session, parse};

let mut session = Session::default();
for statement in parse("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\
                        INSERT INTO users VALUES (1, 'Alice');\
                        SELECT name FROM users WHERE id = 1;")? {
    match session.execute(&statement)? {
        Output::Rows { columns, rows } => println!("{columns:?}: {rows:?}"),
        output => println!("{output:?}"),
    }
}
```

`tokenize(source)` exposes tokens and source ranges for inspection.
`parse(source)` returns all statements or one `SqlError`. `Session::execute`
accepts a parsed statement and returns `Created`, `Affected(count)`, or
`Rows { columns, rows }`; semantic failures return `ExecutionError`.

## Terminal

Run `cargo run -p tinydb-cli`, enter statements ending in `;`, and exit with
Ctrl-D. The prompt accepts multiline input. It also accepts SQL from stdin:

```sh
cargo run -p tinydb-cli <<'SQL'
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
INSERT INTO users VALUES (1, 'Alice', 25), (2, 'Bob', NULL);
UPDATE users SET age = 30 WHERE id = 1;
SELECT name, age FROM users WHERE age >= 18 OR age IS NULL;
DELETE FROM users WHERE id = 2;
SELECT * FROM users;
SQL
```

Each process starts with an empty session; the CLI accepts no database path.
Results use tab-separated headers and values, with text quoted and control
characters escaped. Errors go to stderr with source positions and a caret for
syntax errors. Positions refer to the pending input buffer. The terminal
continues with later input after errors and exits with a failure status if any
error occurred. Each complete input chunk is parsed before it executes; a
syntax error prevents that chunk from running. Semantic errors leave that
statement unchanged and allow later parsed statements to run.

Projections currently accept column names or `*`. UPDATE assignments accept
literals. Joins, aliases, ordering, aggregation, arithmetic, persistent tables,
and the target `Database::open` API remain future work.

[Architecture](architecture.md) · [Milestone 4](roadmap/04-sql-language.md)
