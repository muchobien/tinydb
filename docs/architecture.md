# Architecture

Planned architecture for TinyDB:

```text
CLI / Rust API
      |
SQL Lexer & Parser -> AST
      |
Query Planner & Executor
      |
Record Manager & B+ Tree Indexes
      |
Buffer Pool
      |
Pager & Transaction Manager
      |
app.db + app.db-wal
```

## Implemented: storage fundamentals

The `tinydb` library currently exposes `storage::Pager`, `Page`, `PageId`, and
`PAGE_SIZE`. The pager owns a read/write file and its decoded header. It seeks to
physical page offsets and uses exact reads and complete writes. There is no
buffer pool or record interpretation yet.

Page 0 contains the versioned header. Data pages start at ID 1 and are appended
as zero-filled blocks. Opening validates the header and exact file length before
allowing access. Call `sync()` to persist writes; file locking and atomic crash
recovery remain future work.

See [Storage format](storage-format.md) for the byte layout, invariants, API, and
failure behavior, and [pager tests](../crates/tinydb/tests/pager.rs) for examples.

[Documentation index](README.md)
