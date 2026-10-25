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
and live record. This first heap has no catalog or overflow pages; indexes own
separate files.

See [Record storage](record-storage.md) for layouts, invariants, tradeoffs, and
failure behavior. The [integration tests](../crates/tinydb/tests/record_store.rs)
exercise thousands of records, compaction, stable IDs, and reopening.

## Implemented: B+ tree indexes

`index::BPlusTree` maps unique `i64` keys to physical `RecordId` values. It owns
a pager file with leaf and internal pages, keeping its root at page 1. Internal
nodes route point lookups with binary search; linked leaves support ordered
range scans. Inserts split overflowing nodes and grow the root. Deletes borrow
from siblings or merge nodes, propagate changed minimum keys up the search
path, and contract the root when needed. Retired pages stay allocated.

Opening validates the entire tree, including separator values, occupancy,
equal leaf depths, forward links, and unreachable pages. Subsequent point
operations follow a root-to-leaf path. Index values refer to a separate heap;
callers coordinate heap and index mutations explicitly. A shared catalog and
atomic heap/index transactions remain future work.

See [B+ tree indexes](b-plus-tree.md) for byte layouts and failure semantics,
the [implementation](../crates/tinydb/src/index/btree.rs), and
[integration tests](../crates/tinydb/tests/btree.rs).

[Documentation index](README.md)
