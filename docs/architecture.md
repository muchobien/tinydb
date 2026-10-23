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
buffer pool yet; page contents are interpreted by the record-storage layer.

Page 0 contains the versioned header. Data pages start at ID 1 and are appended
as zero-filled blocks. Opening validates the header and exact file length before
allowing access. Call `sync()` to persist writes; file locking and atomic crash
recovery remain future work.

See [Storage format](storage-format.md) for the byte layout, invariants, API, and
failure behavior, and [pager tests](../crates/tinydb/tests/pager.rs) for examples.

## Implemented: records and slotted pages

`storage::Record` encodes self-describing null, integer, and text values.
`SlottedPage` manages a directory of stable slots and packed variable-length
payloads within one page. Mutations compact payloads while keeping slot IDs
unchanged. Deleted slots remain tombstones and are never reused.

`RecordStore` owns a pager and exposes insert/read/update/delete using
`RecordId { page_id, slot_id }`. Insertion scans pages for capacity before
allocating. Lookup reads a single page. Updates stay on the original page and
fail without modification when growth cannot fit. Opening validates every page
and live record. This first heap has no catalog, index, or overflow pages.

See [Record storage](record-storage.md) for layouts, invariants, tradeoffs, and
failure behavior. The [integration tests](../crates/tinydb/tests/record_store.rs)
exercise thousands of records, compaction, stable IDs, and reopening.

[Documentation index](README.md)
