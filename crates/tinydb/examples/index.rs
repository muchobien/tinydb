use std::io;
use std::path::PathBuf;

use tinydb::index::BPlusTree;
use tinydb::storage::{Record, RecordStore, Value};

fn main() -> io::Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let paths = arguments
        .next()
        .zip(arguments.next())
        .filter(|_| arguments.next().is_none());
    let (heap_path, index_path) = paths.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: cargo run -p tinydb --example index -- <new-heap-path> <new-index-path>",
        )
    })?;
    let heap_path = PathBuf::from(heap_path);
    let index_path = PathBuf::from(index_path);
    let mut heap = RecordStore::create(&heap_path)?;
    let mut index = BPlusTree::create(&index_path)?;
    for key in (0..1000).rev() {
        let record = Record::new(vec![Value::Integer(key), Value::Text(format!("row {key}"))]);
        let id = heap.insert(&record)?;
        index.insert(key, id)?;
    }
    let deleted = index.delete(500)?;
    heap.delete(deleted)?;
    heap.sync()?;
    index.sync()?;
    drop(heap);
    drop(index);

    let mut heap = RecordStore::open(&heap_path)?;
    let mut index = BPlusTree::open(&index_path)?;
    println!(
        "Index: {} ({} allocated pages)",
        index_path.display(),
        index.page_count()
    );
    if let Some(id) = index.get(42)? {
        println!("Key 42: {:?}", heap.read(id)?.values);
    }
    for (key, id) in index.range(498..=502)? {
        println!("Key {key}: {:?}", heap.read(id)?.values);
    }
    assert_eq!(index.get(500)?, None);
    Ok(())
}
