# Milestone 5 — Relational execution (2–3 weeks)

**Learn:** How SQL operations access stored records.

- [x] Persistent schema catalog
- [x] Sequential table scans and index scans
- [x] Filtering and projections
- [x] Joins
- [x] Ordering and basic aggregation

**Deliverable:** Execute queries over multiple persistent tables.

## Acceptance criteria

- [x] Persists and validates table schemas and typed records across reopening
- [x] Maintains INTEGER PRIMARY KEY indexes across inserts, updates, and deletes
- [x] Matches indexed point/range results against forced sequential scans
- [x] Exposes validated scan plans and preserves integer endpoint bounds
- [x] Resolves qualified columns and aliases for inner joins, including self-joins
- [x] Supports ORDER BY, GROUP BY, COUNT, SUM, MIN, and MAX with NULL handling
- [x] Rejects semantic/capacity errors before writes and detects heap/index mismatches
- [x] Retains schemas and query results across terminal processes

## Implementation

- [Catalog, execution, APIs, and failure behavior](../relational-execution.md)
- [Persistent database](../../crates/tinydb/src/execution/database.rs)
- [Scan planner](../../crates/tinydb/src/execution/planner.rs)
- [Relational query evaluator](../../crates/tinydb/src/execution/query.rs)
- [Database tests](../../crates/tinydb/tests/database.rs)
- [Query tests](../../crates/tinydb/tests/execution.rs)
- [Terminal tests](../../crates/tinydb-cli/tests/terminal.rs)

Run `cargo run -p tinydb-cli -- app-data` to create or reopen a database
directory. Omit the path for in-memory execution. Run `cargo test --workspace`
for the integration suite. [Milestone 6](06-transactions-and-durability.md) adds
atomic transactions, WAL/recovery, and exclusive ownership; cost-based
optimization remains Milestone 8.

---

[Roadmap](README.md) · [Previous milestone](04-sql-language.md) · [Next milestone](06-transactions-and-durability.md)
