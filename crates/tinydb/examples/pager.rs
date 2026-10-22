//! Create a small database, persist a page, and read it back after reopening.

use std::io;
use std::path::PathBuf;

use tinydb::storage::{PAGE_SIZE, Pager};

fn main() -> io::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: cargo run -p tinydb --example pager -- <new-database-path>",
            )
        })?;

    let mut pager = Pager::create(&path)?;
    let id = pager.allocate_page()?;
    let message = b"Hello, TinyDB!";
    let mut page = [0; PAGE_SIZE];
    page[..message.len()].copy_from_slice(message);
    pager.write_page(id, &page)?;
    pager.sync()?;
    drop(pager);

    let mut pager = Pager::open(&path)?;
    let page = pager.read_page(id)?;
    println!("Database: {}", path.display());
    println!("Data pages: {}", pager.page_count());
    println!(
        "Page {id}: {}",
        String::from_utf8_lossy(&page[..message.len()])
    );
    Ok(())
}
