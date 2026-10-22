# Milestone 1 — Storage fundamentals (1–2 weeks)

**Learn:** How bytes become persistent data.

- [x] Binary encoding and endianness
- [x] Versioned database header and file format
- [x] Fixed 4096-byte pages
- [x] Page allocation and disk I/O
- [x] File reopening and validation

**Deliverable:** Create, reopen, and validate a database file.

## Acceptance criteria

- [x] Creates a new database file with a valid header
- [x] Allocates fixed-size pages with stable page IDs
- [x] Reads and writes arbitrary page contents
- [x] Persists data across process restarts
- [x] Rejects out-of-range page access
- [x] Detects malformed headers and truncated files
- [x] Passes tests for multiple pages and reopening

## First task

Design the binary database file header, including:

- Magic bytes identifying a TinyDB file
- File format version
- Page size
- Page count or other necessary metadata
- Reserved bytes for future evolution

Then implement a Rust pager capable of creating, reading, writing, and reopening a database file. Include tests before proceeding to record storage.

## Implementation

- [Storage format and design](../storage-format.md)
- [Pager](../../crates/tinydb/src/storage/pager.rs)
- [Header encoding and validation](../../crates/tinydb/src/storage/header.rs)
- [Integration tests](../../crates/tinydb/tests/pager.rs), including a separate writer process
- [Runnable example](../../crates/tinydb/examples/pager.rs)

Run `cargo test --workspace` from the repository root. See the
[project README](../../README.md#current-implementation) to create and inspect an
example database. Transactions, file locking, and crash recovery are not part of
this milestone.

---

[Roadmap](README.md) · [Next milestone](02-records-and-slotted-pages.md)
