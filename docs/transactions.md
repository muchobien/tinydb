# Transactions and durability

Milestone 6 adds BEGIN, COMMIT, ROLLBACK, exclusive database ownership, and
write-ahead recovery across the catalog, heap, and index files. The transaction
boundary lives in `Database`; direct Pager, RecordStore, and BPlusTree users
retain their original direct-write contracts.

## SQL and ownership

```sql
BEGIN TRANSACTION;
UPDATE accounts SET balance = 90 WHERE id = 1;
UPDATE accounts SET balance = 60 WHERE id = 2;
COMMIT;

BEGIN;
DELETE FROM accounts;
ROLLBACK;
```

TRANSACTION is optional after each control command. A second BEGIN during an
active transaction is an error. COMMIT/ROLLBACK require an active transaction.
Transactions cover schema creation and all row mutations; queries see their own
pending changes, including new tables and index entries. Semantic or capacity
errors leave the failed statement unchanged and keep the transaction active.
Earlier successful statements can still be committed or rolled back. Nested
transactions, savepoints, and transaction mode keywords are outside this subset.

Each mutation outside an explicit transaction runs in its own automatic
transaction. A SQL script itself is not automatically one transaction. Dropping
the handle or exiting the terminal with an active transaction discards pending
changes. `sync()` flushes the current view; COMMIT publishes transaction changes.
In-memory Session supports the same control commands using a table snapshot,
without filesystem locking or recovery.

Every persistent Database handle holds an exclusive OS lock on `.lock` for its
entire lifetime, including idle and read-only operations. Another Database or
Journal owner receives `WouldBlock` immediately. Process termination releases the
lock; the lock file stays in place and is reused. The implementation uses
[Rust File::try_lock](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).
Raw filesystem access and direct storage APIs must respect the same ownership
contract. Concurrent readers are not implemented.

## Staged state

BEGIN copies canonical database files into the reserved `.transaction`
directory. SQL executes against that copy using the existing catalog, heap, and
index code. Live files stay unchanged until commit. ROLLBACK closes staged
handles and removes the staging directory. Opening a database discards abandoned
staging after journal recovery, before validating the live catalog.

The database namespace now reserves `.lock`, `wal.log`, `.transaction`, and
`.checkpoint.tmp`. Control/image targets reject symlinks and non-regular files;
staging requires a real directory. On Unix, new control/checkpoint files use
mode 0600 and staging directories use mode 0700. SQL names remain catalog data;
only `catalog.db`, `table-N.db`, and `index-N.db` enter the journal, with canonical
positive decimal IDs.

## WAL format, version 1

`wal.log` contains at most one transaction and is empty after a successful
checkpoint. It contains complete after-images of the managed files. All numeric
fields are little-endian. CRC fields use reflected CRC-32 with polynomial
`0xedb88320`, initial state `0xffffffff`, and final bit inversion.

| Header offset | Bytes | Field |
| --- | --- | --- |
| 0 | 8 | `TDBWAL1\0` |
| 8 | 8 | File-image count (`u64`), nonzero |
| 16 | 4 | CRC-32 of header bytes 0–15 |

Each file image has this header followed by its payload:

| Image offset | Bytes | Field |
| --- | --- | --- |
| 0 | 2 | Filename byte length (`u16`) |
| 2 | 8 | File byte length (`u64`), at least one complete page |
| 10 | 4 | CRC-32 of filename bytes plus file bytes |
| 14 | 4 | CRC-32 of image-header bytes 0–13 |
| 18 | Variable | UTF-8 filename followed by complete file bytes |

Filenames are unique and restricted to managed names. The catalog must be
present. File lengths are multiples of 4096. Header checksums protect lengths
before payload slicing; decoded lengths are checked for overflow
and truncation.

The commit record follows the last image:

| Commit offset | Bytes | Field |
| --- | --- | --- |
| 0 | 8 | `TDBCMT1\0` |
| 8 | 8 | Byte length of the entire WAL body before this record |
| 16 | 4 | CRC-32 of that body |
| 20 | 4 | CRC-32 of commit-record bytes 0–19 |

The commit record must be complete and be the exact end of the file. Unknown
magic, checksum failures, duplicate/invalid names, and trailing bytes fail with
InvalidData while preserving the WAL. A valid incomplete prefix without a
complete commit record is discarded; the live files were never changed.
Checksums detect accidental corruption rather than authenticate input.

## Commit ordering and recovery

COMMIT performs these steps while holding the lock:

1. Synchronize staged catalog, heaps, indexes, and staging directory; close the
   live and staged database handles.
2. Write the WAL header and file images, then synchronize the WAL body.
3. Append the commit record and synchronize it. This is the durable commit point.
4. Validate the complete journal and write each image to `.checkpoint.tmp`.
   Synchronize that file before renaming it over its live target.
5. Synchronize the live database directory after all replacements.
6. Truncate the WAL to zero and synchronize it, reopen the database state, and
   remove staging.

Opening acquires the lock and recovers before opening any catalog, heap, or
index handles. Complete committed images are replayed in full, even when a
previous checkpoint already replaced some files. Replaying the same images is
idempotent. Only after all replacements and the directory are synchronized can
the WAL be cleared. An interrupted checkpoint therefore retains the complete
redo source. Incomplete transactions have no live-file changes to undo.

Storage or synchronization failures poison the Database handle. Discard it and
reopen to run recovery. A COMMIT that reports an I/O failure can already have
passed the durable commit point; reopening determines the recovered outcome.
This contract relies on the local filesystem honoring file synchronization,
directory synchronization, and atomic file replacement.

This first design copies and logs whole files, materializes the WAL in memory,
and checkpoints every commit. Work and temporary space scale with database size,
including for a single-row automatic transaction. Page-granular logging,
concurrent readers, and deferred checkpointing remain future work. The
[WAL background reference](https://www.sqlite.org/wal.html) explains the commit
record and checkpoint concepts.

## APIs and inspection

Use `Database::execute("BEGIN; ...; COMMIT")`, or pass parsed control statements
to `execute_statement`. Results include Begun, Committed, and RolledBack. The CLI
prints BEGIN, COMMIT, and ROLLBACK for those results.

The lower-level `transaction::Journal` owns the same exclusive lock.
`write_transaction(staged_directory)` writes and synchronizes a committed WAL
without checkpointing; it requires an empty journal. `recover()` validates,
replays, and checkpoints the journal, returning whether it replayed a commit.
Callers supply a consistent, complete staged database and hold no competing
Database handle. Database normally coordinates these operations automatically.

To inspect a committed WAL and then recover it, provide a new directory:

```sh
cargo run -p tinydb --example recovery -- /tmp/tinydb-recovery
hexdump -C /tmp/tinydb-recovery/committed-wal.bin
```

The example archives a durable commit before checkpointing, reopens the database
to recover its rows and new table, and demonstrates rollback. The
[integration tests](../crates/tinydb/tests/transactions.rs) cover incomplete WAL
prefixes, corrupt records, repeated replay, partial checkpoints, failed commit
cleanup, locking, abrupt process exits, and a killed active writer.

[Architecture](architecture.md) · [Milestone 6](roadmap/06-transactions-and-durability.md)
