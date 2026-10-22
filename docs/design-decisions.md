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
| Records | Heap/slotted pages | Variable-length record management |
| SQL parser | Handwritten | Understand lexing and parsing |
| Durability | WAL | Learn atomicity and crash recovery |
| Concurrency | Single writer | Keep the first implementation manageable |
| Dependencies | Minimal | Avoid hiding important internals |

Avoid `unsafe` Rust unless a specific need justifies it. Never serialize raw Rust struct memory directly to disk.

The [version 1 storage format](storage-format.md) specifies the implemented
header and pager. Explicit synchronization is available now; atomic writes,
file locking, and recovery are deferred to their milestones.

[Documentation index](README.md)
