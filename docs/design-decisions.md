# Design decisions

| Area | Choice | Why |
| --- | --- | --- |
| Language | Stable Rust | Memory safety and low-level control |
| Page size | 4096 bytes | Familiar fixed-size storage abstraction |
| File format | Custom binary | Learn serialization and compatibility |
| Byte order | Explicit little-endian | Portable, predictable encoding |
| Metadata | Reserve page 0; data-page IDs start at 1 | Keep data-page offsets equal to ID times page size |
| Opening files | Validate version, reserved bytes, and exact file length | Detect unsupported formats, truncation, and inconsistent page counts |
| Storage errors | Standard I/O errors | Keep the first pager API small and preserve underlying filesystem errors |
| Index | B+ tree | Efficient point and range queries |
| Index keys | Unique signed i64 values mapped to RecordId | Start with fixed-width ordering before schema-aware keys |
| Index root | Fixed page 1 in a separate pager file | Persist the root without changing the existing header or heap layout |
| Index deletion | Borrow, merge, and mark removed pages retired | Maintain occupancy while deferring page reuse |
| Records | Heap/slotted pages | Variable-length record management |
| Record values | Tagged null, i64 integer, and UTF-8 text | Decode records without depending on the future SQL schema layer |
| Record IDs | Physical page ID plus permanent slot ID | Keep IDs stable when payload bytes move during compaction |
| Deleted slots | Tombstones; never reuse slot numbers | Prevent deleted IDs from aliasing newly inserted records |
| Compaction | Repack payloads on every page mutation | Reclaim holes with simple, bounded page-sized work |
| Record growth | Single-page records; reject updates that cannot fit | Preserve IDs without adding overflow chains or forwarding yet |
| SQL parser | Handwritten | Understand lexing and parsing |
| SQL values | Existing INTEGER, TEXT, and NULL values with strict types | Keep the first language aligned with record encoding |
| SQL predicates | Comparisons, NULL checks, and three-valued boolean logic | Define filtering before storage-backed execution |
| Initial SQL execution | In-memory session with statement validation before mutation | Make terminal SQL usable before the persistent catalog milestone |
| SQL diagnostics | UTF-8 byte spans with line and character columns | Locate lexical and syntax errors in multiline input |
| Durability | WAL | Learn atomicity and crash recovery |
| Concurrency | Single writer | Keep the first implementation manageable |
| Dependencies | Minimal | Avoid hiding important internals |

Avoid `unsafe` Rust unless a specific need justifies it. Never serialize raw Rust struct memory directly to disk.

The [version 1 storage format](storage-format.md) specifies the implemented
header and pager. Explicit synchronization is available now; atomic writes,
file locking, and recovery are deferred to their milestones.

The [record storage format](record-storage.md) specifies the record codec and
slotted-page layout. Initial insertion scans pages linearly for space; a
free-space map can be added after a demonstrated need.

The [B+ tree format](b-plus-tree.md) specifies index nodes and whole-tree
validation. Callers maintain heap/index consistency explicitly until a shared
catalog and transactions coordinate both structures.

[Documentation index](README.md)
