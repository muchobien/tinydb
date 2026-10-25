# Relational execution

Milestone 5 connects SQL to persistent schemas, record heaps, and B+ tree
indexes. `Database::create(path)` exclusively creates a database directory;
`Database::open(path)` requires an existing database and validates its catalog,
records, and indexes. The default terminal mode and `sql::Session` still offer
in-memory execution using the same SQL evaluator.

## Catalog and files

Each database directory contains:

| File | Contents |
| --- | --- |
| `catalog.db` | RecordStore containing one schema record per table |
| `table-N.db` | RecordStore heap for positive numeric table ID N |
| `index-N.db` | BPlusTree for an INTEGER PRIMARY KEY, when present |

SQL names never become paths. Quoted names containing dots, slashes, or reserved
words remain catalog values. Table IDs grow monotonically from 1. An existing
path is never overwritten by create. Opening does not create missing files.

Catalog records use the existing tagged record encoding:

```text
INTEGER(1), INTEGER(table_id), TEXT(table_name),
repeated [TEXT(column_name), INTEGER(type), INTEGER(not_null), INTEGER(primary_key)]
```

The first value is catalog format version 1. Type 0 is INTEGER and type 1 is
TEXT. Constraint flags are 0 or 1. Each schema and each row must fit in a single
record page; oversized schemas/rows are rejected before writes.

Opening checks catalog versions, field shapes, positive unique table IDs,
unique table/column names, supported types, and primary-key constraints. It
validates every heap row against its schema, including primary-key uniqueness.
For integer primary keys, the entire index must exactly match the sorted keys
and physical record IDs in the heap. Missing files and malformed storage are
errors. Extra files are ignored; this version has no automatic orphan cleanup.

This directory layout reuses the existing version 1 pager format and separate
heap/index ownership. Consolidating these files into one database file remains
future work.

## Execution and scan paths

Persistent queries load rows into the shared SQL session evaluator. The
database caches schemas and file handles, while each statement reads its input
records from storage. `RecordStore::scan()` returns live `(RecordId, Record)`
pairs in physical page/slot order, skipping tombstones.

Single-table SELECT predicates on an INTEGER PRIMARY KEY use its B+ tree for
`=`, `<`, `<=`, `>`, and `>=` comparisons with integer literals. Comparisons can
place the literal on either side. AND predicates intersect index bounds, then
the full WHERE expression filters candidate rows. Bounds preserve inclusive and
exclusive endpoints without arithmetic at integer extremes. Other predicates,
text primary keys, and joins use sequential heap scans. OR/NOT conditions are
not independently pushed into indexes; an enclosing AND can still contribute
an independent key bound.

`Database::plan(sql)` returns `ScanPlan` values with `Sequential` or
`PrimaryKey { lower, upper }` access paths. Planning validates bindings and types
without reading table rows. This is a small rule-based choice, with no cost
model, statistics, or join reordering.

Inner joins use nested loops and evaluate ON against the fields available at
each join. Qualified names and `AS` table aliases resolve columns; an ambiguous
unqualified name is an error, including on empty inputs. WHERE runs after the
joins. Results are materialized in memory.

GROUP BY partitions filtered rows by column values, with NULL values forming a
group. COUNT(*) counts all rows; COUNT(column) skips NULL. SUM accepts INTEGER
columns, skips NULL, and rejects signed overflow. MIN/MAX accept INTEGER or TEXT
columns and skip NULL. Empty ungrouped aggregation returns one row: COUNT is 0
and the other aggregates are NULL. Empty grouped input produces no rows.
Selected non-aggregate columns must appear in GROUP BY.

ORDER BY accepts input columns, including unprojected columns, or output names
and explicit aliases. Grouped queries may order by grouping columns or output
names. Multiple keys support ASC and DESC. NULL sorts first ascending and last
descending; text uses case-sensitive Rust string order. Without ORDER BY, row
order is unspecified and can change with the access path. Outer joins, HAVING,
AVG, DISTINCT, LIMIT, and arithmetic remain outside this subset.

## Mutations and failure behavior

The shared session validates types, nullability, predicates, and final primary
keys before persistent mutations. INSERT/UPDATE pre-encode all new records and
check size limits before changing files. Semantic and capacity errors leave the
files unchanged, including failures late in multirow statements.

INSERT adds heap records and index entries. UPDATE replaces changed heap
records, removes old index entries, and inserts mappings to the replacements.
Changed rows can receive new physical RecordIds and grow onto a different page;
unchanged rows retain their IDs. DELETE removes index entries and retires heap
slots. The lower-level RecordStore update API retains its original same-page,
stable-ID behavior.

The [transaction layer](transactions.md) now stages mutations, synchronizes a
committed WAL, and checkpoints catalog, heap, and index changes together. Each
mutation automatically commits unless enclosed in BEGIN/COMMIT/ROLLBACK. The
database holds an exclusive lock and recovers before validating stored state.
`sync()` flushes the current view; COMMIT publishes pending transaction changes.
An I/O/storage failure poisons the handle and reopening runs recovery. Earlier
successful statements remain applied after a later semantic error; use explicit
transaction control to choose whether to commit or discard the pending group.

## API and terminal

```rust
use tinydb::Database;

let mut database = Database::create("app-data")?;
database.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
                  INSERT INTO users VALUES (1, 'Alice');")?;
let rows = database.query("SELECT name FROM users WHERE id = 1")?;
let plan = database.plan("SELECT * FROM users WHERE id >= 1")?;
```

`execute(sql)` returns one Output per statement. `query(sql)` accepts exactly one
SELECT and returns its Output. `execute_statement(&statement)` executes an
already parsed AST. Errors distinguish syntax, execution, and storage failures.

Run `cargo run -p tinydb-cli -- app-data` to create or reopen a database
directory, or omit the path for an in-memory session. Both modes accept
interactive multiline input and piped SQL. Closing and reopening with the same
directory retains schemas and rows.

See the [SQL language guide](sql-language.md),
[database integration tests](../crates/tinydb/tests/database.rs),
[query integration tests](../crates/tinydb/tests/execution.rs), and
[terminal tests](../crates/tinydb-cli/tests/terminal.rs).

[Architecture](architecture.md) · [Milestone 5](roadmap/05-relational-execution.md)
