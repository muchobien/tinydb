# TinyDB documentation

Build a small, embedded relational database in Rust to understand its internals.

- [Architecture](architecture.md) — planned layers and data flow.
- [Storage format](storage-format.md) — version 1 header, page layout, and pager API.
- [Record storage](record-storage.md) — record encoding, slotted pages, and stable IDs.
- [Design decisions](design-decisions.md) — starting technical choices and constraints.
- [Learning process](learning-process.md) — workflow for every milestone.
- [Planned repository structure](repository-structure.md) — modules to add as needed.
- [Roadmap](roadmap/README.md) — estimates and nine milestone plans.
- [References](references.md) — learning materials.

Begin with [storage fundamentals](roadmap/01-storage-fundamentals.md), then
[records and slotted pages](roadmap/02-records-and-slotted-pages.md). Both
milestones are implemented, with design notes, examples, and tests.

[Project overview](../README.md)
