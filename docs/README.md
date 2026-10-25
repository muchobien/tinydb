# TinyDB documentation

Build a small, embedded relational database in Rust to understand its internals.

- [Architecture](architecture.md) — planned layers and data flow.
- [Storage format](storage-format.md) — version 1 header, page layout, and pager API.
- [Record storage](record-storage.md) — record encoding, slotted pages, and stable IDs.
- [B+ tree indexes](b-plus-tree.md) — persistent integer indexes, range scans, and rebalancing.
- [SQL language](sql-language.md) — lexer, AST, parser, and in-memory terminal execution.
- [Design decisions](design-decisions.md) — starting technical choices and constraints.
- [Learning process](learning-process.md) — workflow for every milestone.
- [Planned repository structure](repository-structure.md) — modules to add as needed.
- [Roadmap](roadmap/README.md) — estimates and nine milestone plans.
- [References](references.md) — learning materials.

The first four milestones are implemented: [storage fundamentals](roadmap/01-storage-fundamentals.md),
[records and slotted pages](roadmap/02-records-and-slotted-pages.md), and
[B+ tree indexes](roadmap/03-b-plus-tree-indexes.md), and
[SQL language](roadmap/04-sql-language.md), with design notes and tests.
Continue with [relational execution](roadmap/05-relational-execution.md).

[Project overview](../README.md)
