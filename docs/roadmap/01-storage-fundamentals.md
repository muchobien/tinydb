# Milestone 1 — Storage fundamentals (1–2 weeks)

**Learn:** How bytes become persistent data.

- [ ] Binary encoding and endianness
- [ ] Versioned database header and file format
- [ ] Fixed 4096-byte pages
- [ ] Page allocation and disk I/O
- [ ] File reopening and validation

**Deliverable:** Create, reopen, and validate a database file.

## Acceptance criteria

- [ ] Creates a new database file with a valid header
- [ ] Allocates fixed-size pages with stable page IDs
- [ ] Reads and writes arbitrary page contents
- [ ] Persists data across process restarts
- [ ] Rejects out-of-range page access
- [ ] Detects malformed headers and truncated files
- [ ] Passes tests for multiple pages and reopening

## First task

Design the binary database file header, including:

- Magic bytes identifying a TinyDB file
- File format version
- Page size
- Page count or other necessary metadata
- Reserved bytes for future evolution

Then implement a Rust pager capable of creating, reading, writing, and reopening a database file. Include tests before proceeding to record storage.

---

[Roadmap](README.md) · [Next milestone](02-records-and-slotted-pages.md)
