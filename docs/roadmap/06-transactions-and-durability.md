# Milestone 6 — Transactions and durability (2–3 weeks)

**Learn:** Atomicity, consistency, isolation, durability, and crash recovery.

- [x] `BEGIN`, `COMMIT`, `ROLLBACK`
- [x] Write-ahead log format and commit records
- [x] Crash recovery and checkpointing
- [x] Single-writer locking
- [x] Correct write and sync ordering

**Deliverable:** Recover committed transactions and discard incomplete ones after simulated crashes.

## Acceptance criteria

- [x] Rolls back schemas and rows and supports read-your-writes in both SQL modes
- [x] Keeps live files unchanged until a durable commit record
- [x] Commits individual mutations automatically outside explicit transactions
- [x] Serializes database owners with an OS lock across handles and processes
- [x] Detects malformed WAL headers, images, commit records, and trailing bytes
- [x] Discards incomplete WAL prefixes and abandoned transaction staging
- [x] Replays committed WAL images before validating catalog/heap/index state
- [x] Repeats checkpoints safely after partial application or a checkpoint error
- [x] Recovers after abrupt process exits and a killed active writer
- [x] Preserves pending transactions after statement validation errors

## Implementation

- [Semantics, WAL byte layout, and sync ordering](../transactions.md)
- [Transaction coordinator](../../crates/tinydb/src/transaction/database.rs)
- [Journal, locking, and recovery](../../crates/tinydb/src/transaction/journal.rs)
- [Integration tests](../../crates/tinydb/tests/transactions.rs)
- [Terminal tests](../../crates/tinydb-cli/tests/terminal.rs)
- [Recovery example](../../crates/tinydb/examples/recovery.rs)

Run `cargo test --workspace`. To archive a committed WAL and recover it, run
`cargo run -p tinydb --example recovery -- /tmp/tinydb-recovery` with a new
directory. The initial design copies and logs whole files, checkpoints each
commit, and takes an exclusive lock for each Database handle. Concurrent
readers, savepoints, and page-granular logging remain future work.

---

[Roadmap](README.md) · [Previous milestone](05-relational-execution.md) · [Next milestone](07-buffer-pool.md)
