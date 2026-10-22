# TinyDB — Build a SQLite-like Database in Rust

Build a small, embedded relational database from scratch in Rust to understand database internals: disk storage, indexing, SQL execution, transactions, recovery, and query optimization.

This is an educational project, **not** a production-ready SQLite replacement.

### Target usage

```rust
let db = Database::open("app.db")?;

db.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)")?;
db.execute("INSERT INTO users VALUES (1, 'Alice', 25)")?;

let rows = db.query("SELECT * FROM users WHERE id = 1")?;
```

### Target features

- Single-file persistent storage (`app.db`)
- Fixed-size disk pages
- Slotted pages and variable-length records
- B+ tree indexes
- SQL parser and query execution engine
- Transactions, rollback, and write-ahead logging (WAL)
- Buffer pool and page caching
- Basic query optimization
- Interactive SQL CLI
- Single-writer concurrency model

## Documentation

- [Documentation index](docs/README.md)
- [Roadmap and milestone plans](docs/roadmap/README.md)
- [Architecture](docs/architecture.md)
- [Design decisions](docs/design-decisions.md)
- [Learning process](docs/learning-process.md)
- [Planned repository structure](docs/repository-structure.md)
- [References](docs/references.md)

This repository starts with the learning plan. The Rust crates and modules will be added as the milestones are implemented.

Start with the [Milestone 1 first task](docs/roadmap/01-storage-fundamentals.md#first-task).
