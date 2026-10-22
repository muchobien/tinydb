# Architecture

Planned architecture for TinyDB:

```text
CLI / Rust API
      |
SQL Lexer & Parser -> AST
      |
Query Planner & Executor
      |
Record Manager & B+ Tree Indexes
      |
Buffer Pool
      |
Pager & Transaction Manager
      |
app.db + app.db-wal
```

Update this document as subsystem designs are implemented. Record the binary layouts and APIs in short subsystem design notes.

[Documentation index](README.md)
