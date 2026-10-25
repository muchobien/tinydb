# Records and slotted pages

Milestone 2 adds a record codec, slotted pages, and a `RecordStore` over the pager.
It uses the existing version 1 database header. A record-store file dedicates
every data page to the slotted-page format below; raw pager files are only
compatible if their pages contain valid slotted pages.

## Record encoding

A `Record` contains an ordered list of `Value::Null`, `Value::Integer(i64)`, or
`Value::Text(String)`. All numbers are little-endian. Schema/type checking will
come with SQL; records currently carry their own value tags.

| Field | Encoding |
| --- | --- |
| Number of values | `u16` |
| Null | Tag `0` |
| Integer | Tag `1`, followed by an `i64` |
| Text | Tag `2`, followed by byte length (`u32`) and UTF-8 bytes |

The decoder rejects unknown tags, truncated fields, invalid UTF-8, and trailing
bytes. An empty record is valid; its encoding is two zero bytes. Oversized field
counts or text lengths are rejected by the encoder. There are no overflow pages:
the largest record that fits on an empty page is 4076 bytes, including encoding.

## Slotted-page layout

| Byte offset | Length | Field |
| --- | --- | --- |
| 0 | 4 | Magic/version bytes `SLT1` |
| 4 | 2 | Number of slot entries (`u16`) |
| 6 | 2 | Beginning of packed record data (`u16`), initially 4096 |
| 8 | 8 | Reserved, zero |
| 16 | 4 per slot | Record offset (`u16`) and length (`u16`) |

The slot directory grows toward the end of the page. Record bytes are packed
from the end toward the directory. The free region between them is zero-filled.
A slot is a tombstone when both offset and length are zero; live records must
have nonzero lengths. Slot IDs start at zero and are never reused or renumbered.
At most 1020 slots fit in a page, including tombstones.

Every successful mutation rebuilds packed data and offsets in a fresh page-sized
buffer. This simple approach reclaims deleted and shortened payloads immediately
and preserves the original page on capacity/validation errors. Tombstones retain
their four-byte directory entries. Reads validate directory bounds, offsets,
lengths, reserved/free bytes, overlapping payloads, and gaps in packed data.

`SlottedPage` operates on bytes; `RecordStore` adds record decoding. Corruption
of an otherwise structurally valid value is not detectable without checksums.

## Record store API

- `create(path)` creates a database exclusively. `open(path)` validates all
  slotted pages and live record encodings before returning the store.
- `insert(&record)` scans pages for sufficient space, or appends a new page,
  and returns `RecordId { page_id, slot_id }`.
- `read(id)`, `update(id, &record)`, and `delete(id)` access a stable slot.
- `page_count()` returns the number of data pages; `sync()` persists changes.

Updates may grow within the same page. If insufficient space remains, return
`StorageFull` and leave the original record unchanged. Oversized inserts also
return `StorageFull` without allocating a page. Moving a record to another page
while retaining its ID requires forwarding or indirection, which is deferred.
Missing/deleted slots report `NotFound`; invalid page IDs retain the pager's
`InvalidInput` errors. Malformed stored pages/records report `InvalidData`.

Use one store per file and call `sync()` before relying on persistence. The
pager's write-error and crash-recovery limitations still apply. There is no
transaction, table catalog, integrated index, free-space map, or page deallocation yet.
Insertion scans linearly; direct record lookup reads one page.

[B+ tree indexes](b-plus-tree.md) map integer keys to these record IDs in a
separate file. Direct callers explicitly maintain both structures; the
[Database API](relational-execution.md) maintains them for persistent SQL tables.

## Learning notes

Stable IDs refer to slots rather than byte offsets: compaction changes offsets,
but leaves `(page_id, slot_id)` intact. Tombstones prevent a deleted ID from
silently naming a new record. This costs directory space, even when all payloads
are deleted. Later reclamation would need generations or another identity scheme.

[Storage format](storage-format.md) · [Milestone 2](roadmap/02-records-and-slotted-pages.md)
