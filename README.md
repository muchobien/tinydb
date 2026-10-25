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

## Current implementation

Milestone 1 implements a versioned binary header and a pager for creating,
allocating, reading, writing, syncing, and reopening 4096-byte pages. Milestone 2
adds typed record encoding, slotted pages, and a record store with stable IDs,
updates, deletion, and space reclamation. Milestone 3 adds persistent B+ tree
indexes with unique integer keys, point lookups, range scans, and deletion with
rebalancing. SQL and the `Database` API shown above are future milestones.

With stable Rust installed, run:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Create a database at a **new file path**, write a page, and read it back:

```sh
cargo run -p tinydb --example pager -- /tmp/tinydb-example.db
hexdump -C /tmp/tinydb-example.db | head
```

Read the [storage format and pager design](docs/storage-format.md), then explore
the [Milestone 1 implementation](crates/tinydb/src/storage/pager.rs) and
[tests](crates/tinydb/tests/pager.rs). The pager requires a single owner per file;
transactions and crash recovery are planned for Milestone 6.

To try structured records, use a separate **new file path**:

```sh
cargo run -p tinydb --example records -- /tmp/tinydb-records.db
```

The example inserts, updates, and deletes records, then reopens the file. Read
the [record storage design](docs/record-storage.md) for the binary format,
stable-ID behavior, and single-page record/update limits.

To index real records, provide two separate **new file paths**:

```sh
cargo run -p tinydb --example index -- /tmp/tinydb-index-heap.db /tmp/tinydb-index.db
```

The example builds an index, deletes a key and its record, then reopens both
files for point lookups and an ordered range scan. Read the
[B+ tree design](docs/b-plus-tree.md) for node layouts, splitting, deletion,
validation, and the current separate-file constraint.
