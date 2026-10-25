# Milestone 3 — B+ tree indexes (2–3 weeks)

**Learn:** How databases avoid full-table scans.

- [x] Leaf and internal node formats
- [x] Binary search within nodes
- [x] Node splitting and root growth
- [x] Ordered traversal and range scans
- [x] Deletion, merging, and rebalancing

**Deliverable:** Efficient indexed point lookups and range queries.

## Acceptance criteria

- [x] Persists unique integer keys mapped to stable record IDs
- [x] Grows to three levels with 60,000 ascending or descending inserts
- [x] Matches a reference map across mixed inserts, deletes, and lookups
- [x] Scans linked leaves with inclusive, exclusive, and unbounded endpoints
- [x] Borrows and merges nodes, contracts to an empty root, and accepts new inserts
- [x] Reopens validated trees and survives a separate writer process exiting
- [x] Rejects malformed node bytes, separators, child references, and leaf links
- [x] Resolves indexed IDs to real records in a separate heap

## Implementation

- [Design and byte layouts](../b-plus-tree.md)
- [B+ tree](../../crates/tinydb/src/index/btree.rs)
- [Integration tests](../../crates/tinydb/tests/btree.rs)
- [Runnable heap/index example](../../crates/tinydb/examples/index.rs)

Run `cargo test --workspace`. To create an indexed heap, run
`cargo run -p tinydb --example index -- /tmp/tinydb-index-heap.db /tmp/tinydb-index.db`
with two new file paths. Indexes use separate files; callers coordinate heap
and index mutations. Retired pages remain allocated. Shared-file catalogs,
page reuse, and atomic heap/index transactions remain future work.

---

[Roadmap](README.md) · [Previous milestone](02-records-and-slotted-pages.md) · [Next milestone](04-sql-language.md)
