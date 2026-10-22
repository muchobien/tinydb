# Planned repository structure

Start small, then add modules as needed.

```text
tinydb/
├── Cargo.toml
├── crates/
│   ├── tinydb/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── storage/
│   │       │   ├── pager.rs
│   │       │   ├── page.rs
│   │       │   └── record.rs
│   │       ├── index/
│   │       │   └── btree.rs
│   │       ├── sql/
│   │       │   ├── lexer.rs
│   │       │   ├── parser.rs
│   │       │   └── ast.rs
│   │       ├── execution/
│   │       │   ├── planner.rs
│   │       │   └── executor.rs
│   │       ├── transaction/
│   │       │   ├── wal.rs
│   │       │   └── recovery.rs
│   │       └── buffer/
│   │           └── pool.rs
│   └── tinydb-cli/
│       ├── Cargo.toml
│       └── src/main.rs
├── tests/
├── benches/
└── docs/
```

This is the intended Rust project layout. Add crates and modules when their milestone needs them; the initial repository contains the documentation only.

The learning documentation is organized as:

```text
docs/
├── README.md
├── architecture.md
├── design-decisions.md
├── learning-process.md
├── references.md
├── repository-structure.md
└── roadmap/
    ├── README.md
    ├── 01-storage-fundamentals.md
    ├── 02-records-and-slotted-pages.md
    ├── 03-b-plus-tree-indexes.md
    ├── 04-sql-language.md
    ├── 05-relational-execution.md
    ├── 06-transactions-and-durability.md
    ├── 07-buffer-pool.md
    ├── 08-query-optimization.md
    └── 09-testing-and-benchmarking.md
```

[Documentation index](README.md)
