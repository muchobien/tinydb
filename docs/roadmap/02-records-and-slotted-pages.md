# Milestone 2 — Records and slotted pages (1–2 weeks)

**Learn:** How variable-length records fit into fixed-size pages.

- [x] Record serialization and deserialization
- [x] Page headers and slot directories
- [x] Insert, read, update, and delete records
- [x] Tombstones and free-space management
- [x] Stable record identifiers

**Deliverable:** Store thousands of records and retrieve them by record ID.

## Acceptance criteria

- [x] Round-trips nulls, integer extremes, empty text, and Unicode text
- [x] Rejects malformed record encodings and slotted-page layouts
- [x] Inserts and reads 3,000 variable-length records across pages
- [x] Preserves IDs through updates, compaction, and reopening
- [x] Reclaims deleted/shrunken payload space without reusing slot IDs
- [x] Rejects oversized inserts and updates without changing existing records
- [x] Persists records and tombstones across process restarts

## Implementation

- [Design and byte layouts](../record-storage.md)
- [Record codec](../../crates/tinydb/src/storage/record.rs)
- [Slotted pages](../../crates/tinydb/src/storage/slotted_page.rs)
- [Record store](../../crates/tinydb/src/storage/record_store.rs)
- [Codec and page tests](../../crates/tinydb/tests/records.rs)
- [Record-store integration tests](../../crates/tinydb/tests/record_store.rs)
- [Runnable example](../../crates/tinydb/examples/records.rs)

Run `cargo test --workspace`. To create and inspect structured records, run
`cargo run -p tinydb --example records -- /tmp/tinydb-records.db` with a new
file path. Records must fit on one page; updates stay on their original page.
Transactions and crash recovery remain future work.

---

[Roadmap](README.md) · [Previous milestone](01-storage-fundamentals.md) · [Next milestone](03-b-plus-tree-indexes.md)
