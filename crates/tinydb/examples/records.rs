//! Persist structured records while preserving IDs across page compaction.

use std::io;
use std::path::PathBuf;

use tinydb::storage::{Record, RecordStore, Value};

fn main() -> io::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: cargo run -p tinydb --example records -- <new-database-path>",
            )
        })?;
    let mut store = RecordStore::create(&path)?;
    let alice = store.insert(&Record::new(vec![
        Value::Integer(1),
        Value::Text("Alice".into()),
        Value::Null,
    ]))?;
    let bob = store.insert(&Record::new(vec![Value::Text("Bob".into())]))?;
    store.update(
        alice,
        &Record::new(vec![
            Value::Integer(1),
            Value::Text("Alice García".into()),
            Value::Integer(25),
        ]),
    )?;
    store.delete(bob)?;
    let charlie = store.insert(&Record::new(vec![Value::Text("Charlie".into())]))?;
    store.sync()?;
    drop(store);

    let mut store = RecordStore::open(&path)?;
    println!(
        "Database: {} ({} data page)",
        path.display(),
        store.page_count()
    );
    println!("Alice {alice:?}: {:?}", store.read(alice)?.values);
    println!("Charlie {charlie:?}: {:?}", store.read(charlie)?.values);
    println!("Bob's retired ID: {bob:?}");
    assert_eq!(store.read(bob).unwrap_err().kind(), io::ErrorKind::NotFound);
    Ok(())
}
