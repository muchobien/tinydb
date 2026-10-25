# Storage format, version 1

TinyDB stores a database as a sequence of 4096-byte pages. Page 0 is a
metadata page; data pages have stable IDs starting at 1. A new database contains
only page 0. The header's page count counts **data pages**, excluding page 0.

## Header layout

All integer fields use little-endian encoding. Encode individual fields into
bytes rather than serializing a Rust struct's memory.

| Byte offset | Length | Field | Version 1 value |
| --- | --- | --- | --- |
| 0 | 8 | Magic bytes | `54 49 4e 59 44 42 00 00` (`TINYDB\0\0`) |
| 8 | 4 | Format version (`u32`) | 1 |
| 12 | 4 | Page size (`u32`) | 4096 |
| 16 | 8 | Data-page count (`u64`) | Initially 0 |
| 24 | 4072 | Reserved | All zero |

Data page `id` begins at byte `id * 4096`. The
[record-storage layer](record-storage.md) interprets heap data pages as slotted
pages. [B+ tree indexes](b-plus-tree.md) use separate files with their own node
pages; the pager treats all page payloads as opaque bytes. The file length must equal
`(data_page_count + 1) * 4096`. Check arithmetic for overflow before seeking or
accepting an on-disk count. Reject unknown versions, unexpected page sizes,
nonzero reserved bytes, truncated files, and extra bytes. Version 1 has no
checksums, so arbitrary changes to data-page contents cannot be detected.

## Pager API and invariants

- `Pager::create(path)` exclusively creates a database; an existing file is never
  overwritten. `Pager::open(path)` requires an existing, valid database.
- `allocate_page()` appends a zero-filled page, updates the header, and returns
  its ID. IDs are consecutive and are not reused; there is no deallocation yet.
- `read_page(id)` and `write_page(id, &page)` transfer exactly 4096 bytes. Page 0
  and IDs greater than the data-page count are inaccessible through these APIs.
- `page_count()` returns the number of allocated data pages.
- `sync()` calls `File::sync_all()`. Call it before relying on persistence across
  process restarts. Dropping a pager is not a substitute for checking `sync()`.
- File errors use `std::io::Error`; invalid database files report `InvalidData`,
  and invalid page IDs report `InvalidInput`.

Use only one pager/writer for a file at a time. Direct pager operations have no
locking, transactions, atomic allocation, or crash recovery. The
[Database transaction layer](transactions.md) stages SQL changes and adds WAL
recovery across managed files. Direct allocation writes the
new page before its header count; a failed or interrupted write can leave an
invalid file. After a write or sync error, discard the pager and investigate
the file before proceeding. A successful sync is not transaction atomicity.

## Inspecting the bytes

`hexdump -C app.db | head` shows the magic bytes and little-endian header fields.
The tests inspect the serialized header directly and verify reopen behavior,
malformed files, bounds, zero initialization, and persistence between processes.

[Architecture](architecture.md) · [Milestone 1](roadmap/01-storage-fundamentals.md)
