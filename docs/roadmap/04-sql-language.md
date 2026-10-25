# Milestone 4 — SQL language (1–2 weeks)

**Learn:** How text becomes a structured query.

- [x] Tokenizer / lexer
- [x] Handwritten recursive-descent parser
- [x] Abstract syntax tree (AST)
- [x] `CREATE TABLE`, `INSERT`, `SELECT`, `UPDATE`, `DELETE`
- [x] Syntax errors with useful diagnostics

**Deliverable:** Parse and execute basic SQL from a terminal.

## Acceptance criteria

- [x] Parses scripts with quoted values, comments, and source ranges
- [x] Supports INTEGER, TEXT, NULL, column constraints, and multirow INSERT
- [x] Preserves boolean precedence and parentheses in the AST
- [x] Reports lexical/syntax errors with line, column, and byte spans
- [x] Bounds recursive nesting and accepts long flat boolean lists
- [x] Executes CRUD, filtering, and projection in an in-memory session
- [x] Enforces types, nullability, and unique primary keys without partial statement mutations
- [x] Handles multiline and piped terminal input, errors, and EOF

## Implementation

- [Grammar, API, and evaluation semantics](../sql-language.md)
- [Lexer](../../crates/tinydb/src/sql/lexer.rs)
- [Parser](../../crates/tinydb/src/sql/parser.rs) and [AST](../../crates/tinydb/src/sql/ast.rs)
- [In-memory session](../../crates/tinydb/src/sql/session.rs)
- [Terminal](../../crates/tinydb-cli/src/main.rs)
- [SQL tests](../../crates/tinydb/tests/sql.rs) and [terminal tests](../../crates/tinydb-cli/tests/terminal.rs)

Run `cargo run -p tinydb-cli`. Tables live in memory for the process lifetime;
the CLI accepts no database path. Persistent schema/catalog storage, joins,
ordering, aggregation, and query planning remain Milestone 5. Run
`cargo test --workspace` to exercise all storage, indexing, SQL, and terminal
integration coverage.

---

[Roadmap](README.md) · [Previous milestone](03-b-plus-tree-indexes.md) · [Next milestone](05-relational-execution.md)
