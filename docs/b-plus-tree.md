# B+ tree indexes

The index maps unique signed `i64` keys to `RecordId` values. Internal nodes
contain separator keys and child page IDs; leaves contain the actual mappings.
All leaves have the same depth, and their forward links support ordered scans.
For each separator, the key is exactly the minimum key of its right subtree.
Equality therefore descends into the right child.

## Storage and page layouts

An index owns a separate pager file with the existing version 1 database header.
Page 1 is always the root, initially an empty leaf. Root growth copies its left
half to a new page and replaces page 1 with an internal node. Root contraction
copies its only child into page 1. The heap file is separate because RecordStore
currently interprets every data page as a slotted page. The
[Database catalog](relational-execution.md) coordinates these separate files;
consolidating them into one file remains future work.

All numbers are explicitly little-endian. Every node starts with a 16-byte
header. Bytes 4–5 contain the number of keys (`u16`); bytes 6–7 are zero.

| Node | Magic | Bytes 8–15 | Payload at byte 16 | Capacity |
| --- | --- | --- | --- | --- |
| Leaf | `BTL1` | Next leaf ID, or 0 | Repeated key (`i64`), record page ID (`u64`), slot ID (`u16`) | 226 entries |
| Internal | `BTI1` | Zero | First child (`u64`), then repeated separator (`i64`), right child (`u64`) | 254 keys, 255 children |
| Retired | `BTF1` | Zero | Zero | No entries |

Unused bytes are zero. Child IDs reference allocated index pages; record IDs
refer to the separate heap and require a nonzero page ID. The index does not
verify whether a referenced record exists or automatically track heap changes.

Non-root leaves contain at least 113 entries. Non-root internal nodes contain
at least 127 keys (128 children). An internal root has at least one key.
Overflow splits a leaf into 113/114 entries or an internal node into 127/127
keys with its middle separator promoted. Deletion borrows from an adjacent
sibling when possible, otherwise merges nodes and repairs the parent. Removed
nodes are zeroed and marked retired. The pager does not deallocate or reuse
pages, so repeated growth and shrinkage can increase file size.

## API and validation

`index::BPlusTree` exposes:

- `create(path)` and `open(path)` for exclusive creation and validated reopening.
- `insert(key, record_id)`; duplicate keys return `AlreadyExists` without writes.
- `get(key)` returning `Option<RecordId>`.
- `range(bounds)` returning ordered `(i64, RecordId)` pairs for standard Rust
  range bounds, including inclusive, exclusive, and unbounded endpoints.
  Empty or reversed intervals return an empty vector. Results are materialized
  in memory; lookup descends directly to the first relevant leaf.
- `delete(key)` returning the removed ID, or `NotFound` without writes.
- `page_count()` counting all allocated pages, including retired nodes, and
  `sync()` to synchronize the file.

Opening checks node headers, capacities, sorted unique keys, reserved/free
bytes, record page IDs, child bounds, occupancy, separators, disjoint child
ranges, equal leaf depths, and the exact leaf-link chain. It rejects cycles,
shared children, unreachable live pages, and malformed retired pages. Validation
reads the whole index; subsequent point operations visit a root-to-leaf path.
Tree depth is limited to 64 during validation to bound malformed input.

Use a single owner per file. Invalid files report `InvalidData`; record page 0
reports `InvalidInput`. Writes and synchronization retain the pager's failure
semantics: discard the tree after an I/O write/sync failure. Splits and merges
are not crash-atomic. The [Database transaction layer](transactions.md) provides
atomic commit and recovery for persistent SQL indexes and heaps. Direct tree
users coordinate their own writes. Text/composite/nonunique keys and streaming
cursors remain future work.

[Architecture](architecture.md) · [Milestone 3](roadmap/03-b-plus-tree-indexes.md)
