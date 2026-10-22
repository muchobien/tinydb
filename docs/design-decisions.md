# Design decisions

| Area | Choice | Why |
| --- | --- | --- |
| Language | Stable Rust | Memory safety and low-level control |
| Page size | 4096 bytes | Familiar fixed-size storage abstraction |
| File format | Custom binary | Learn serialization and compatibility |
| Byte order | Explicit little-endian | Portable, predictable encoding |
| Index | B+ tree | Efficient point and range queries |
| Records | Heap/slotted pages | Variable-length record management |
| SQL parser | Handwritten | Understand lexing and parsing |
| Durability | WAL | Learn atomicity and crash recovery |
| Concurrency | Single writer | Keep the first implementation manageable |
| Dependencies | Minimal | Avoid hiding important internals |

Avoid `unsafe` Rust unless a specific need justifies it. Never serialize raw Rust struct memory directly to disk.

[Documentation index](README.md)
