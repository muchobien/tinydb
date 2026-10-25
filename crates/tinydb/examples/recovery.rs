use std::fs;
use std::io;
use std::path::PathBuf;

use tinydb::Database;
use tinydb::transaction::Journal;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments
        .next()
        .map(PathBuf::from)
        .filter(|_| arguments.next().is_none())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: cargo run -p tinydb --example recovery -- <new-directory>",
            )
        })?;
    fs::create_dir(&path)?;
    let home = path.join("database");
    let staged = path.join("staged");
    let mut database = Database::create(&home)?;
    database.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO users VALUES (1, 'old')")?;
    drop(database);
    let mut database = Database::create(&staged)?;
    database.execute("BEGIN; CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO users VALUES (1, 'committed'), (2, 'added'); CREATE TABLE audit (id INTEGER PRIMARY KEY); INSERT INTO audit VALUES (9); COMMIT")?;
    drop(database);
    let mut journal = Journal::open(&home)?;
    journal.write_transaction(&staged)?;
    let archive = path.join("committed-wal.bin");
    fs::copy(home.join("wal.log"), &archive)?;
    println!(
        "Durable commit before checkpoint: {} ({} bytes)",
        archive.display(),
        fs::metadata(&archive)?.len()
    );
    drop(journal);

    let mut database = Database::open(&home)?;
    println!(
        "Recovered users: {:?}",
        database.query("SELECT * FROM users ORDER BY id")?
    );
    println!(
        "Recovered schema: {:?}",
        database.query("SELECT * FROM audit")?
    );
    println!(
        "Checkpointed WAL length: {}",
        fs::metadata(home.join("wal.log"))?.len()
    );
    database.execute("BEGIN; DELETE FROM users; ROLLBACK")?;
    println!(
        "Rows after rollback: {:?}",
        database.query("SELECT COUNT(*) FROM users")?
    );
    Ok(())
}
