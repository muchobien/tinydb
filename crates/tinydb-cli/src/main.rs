use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use tinydb::Database;
use tinydb::sql::{Output, Session, SqlError, TokenKind, parse, tokenize};
use tinydb::storage::Value;

enum Engine {
    Memory(Session),
    Persistent(Database),
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments.next().map(std::path::PathBuf::from);
    if arguments.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: tinydb-cli [database-directory] (reads SQL from stdin)",
        )
        .into());
    }
    let mut engine = if let Some(path) = &path {
        Engine::Persistent(if path.exists() {
            Database::open(path)?
        } else {
            Database::create(path)?
        })
    } else {
        Engine::Memory(Session::default())
    };
    let stdin = io::stdin();
    let interactive = stdin.is_terminal();
    if interactive {
        if let Some(path) = &path {
            println!(
                "TinyDB SQL ({}). End statements with ';'. Ctrl-D exits.",
                path.display()
            );
        } else {
            println!(
                "TinyDB SQL (in memory; tables disappear on exit). End statements with ';'. Ctrl-D exits."
            );
        }
    }
    let mut buffer = String::new();
    let mut success = true;
    loop {
        if interactive {
            if buffer.trim().is_empty() {
                print!("tinydb> ");
            } else {
                print!("   ...> ");
            }
            io::stdout().flush()?;
        }
        if stdin.read_line(&mut buffer)? == 0 {
            return Ok(execute(&buffer, &mut engine) && success);
        }
        match tokenize(&buffer) {
            Ok(tokens) => {
                if let Some(end) = tokens
                    .iter()
                    .rfind(|token| token.kind == TokenKind::Semicolon)
                    .map(|token| token.span.end)
                {
                    let source: String = buffer.drain(..end).collect();
                    success = execute(&source, &mut engine) && success;
                }
            }
            Err(error) if error.message.starts_with("unterminated") => {}
            Err(error) => {
                report(&buffer, &error);
                buffer.clear();
                success = false;
            }
        }
    }
}

fn report(source: &str, error: &SqlError) {
    eprintln!("{error}");
    if let Some(line) = source.split('\n').nth(error.line - 1) {
        eprintln!("{line}\n{}^", " ".repeat(error.column - 1));
    }
}

fn execute(source: &str, engine: &mut Engine) -> bool {
    let statements = match parse(source) {
        Ok(statements) => statements,
        Err(error) => {
            report(source, &error);
            return false;
        }
    };
    let mut success = true;
    for statement in statements {
        let result = match engine {
            Engine::Memory(session) => session
                .execute(&statement)
                .map_err(|error| error.to_string()),
            Engine::Persistent(database) => database
                .execute_statement(&statement)
                .map_err(|error| error.to_string()),
        };
        match result {
            Ok(Output::Created) => println!("CREATE TABLE"),
            Ok(Output::Affected(count)) => println!("{count} row(s) affected"),
            Ok(Output::Rows { columns, rows }) => {
                println!("{}", columns.join("\t"));
                for row in &rows {
                    let values: Vec<_> = row
                        .iter()
                        .map(|value| match value {
                            Value::Null => "NULL".into(),
                            Value::Integer(value) => value.to_string(),
                            Value::Text(value) => format!(
                                "'{}'",
                                value
                                    .replace('\\', "\\\\")
                                    .replace('\t', "\\t")
                                    .replace('\n', "\\n")
                                    .replace('\r', "\\r")
                                    .replace('\'', "''")
                            ),
                        })
                        .collect();
                    println!("{}", values.join("\t"));
                }
                println!("{} row(s)", rows.len());
            }
            Err(error) => {
                eprintln!("{error}");
                success = false;
            }
        }
    }
    success
}
