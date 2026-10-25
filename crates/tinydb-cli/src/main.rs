use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use tinydb::sql::{Output, Session, SqlError, TokenKind, parse, tokenize};
use tinydb::storage::Value;

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

fn run() -> io::Result<bool> {
    if std::env::args_os().len() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: tinydb-cli (reads SQL from stdin; tables live in memory)",
        ));
    }
    let stdin = io::stdin();
    let interactive = stdin.is_terminal();
    if interactive {
        println!(
            "TinyDB SQL (in memory; tables disappear on exit). End statements with ';'. Ctrl-D exits."
        );
    }
    let mut session = Session::default();
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
            return Ok(execute(&buffer, &mut session) && success);
        }
        match tokenize(&buffer) {
            Ok(tokens) => {
                if let Some(end) = tokens
                    .iter()
                    .rfind(|token| token.kind == TokenKind::Semicolon)
                    .map(|token| token.span.end)
                {
                    let source: String = buffer.drain(..end).collect();
                    success = execute(&source, &mut session) && success;
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

fn execute(source: &str, session: &mut Session) -> bool {
    let statements = match parse(source) {
        Ok(statements) => statements,
        Err(error) => {
            report(source, &error);
            return false;
        }
    };
    let mut success = true;
    for statement in statements {
        match session.execute(&statement) {
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
