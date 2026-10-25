use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use crate::execution::{DatabaseError, DatabaseState, ScanPlan};
use crate::sql::{ExecutionError, Output, Statement, parse};

use super::Journal;
use super::journal::managed_files;

pub struct Database {
    path: PathBuf,
    state: Option<DatabaseState>,
    active: Option<DatabaseState>,
    journal: Journal,
    failed: bool,
}

impl Database {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = std::path::absolute(path)?;
        fs::create_dir(&path)?;
        let journal = Journal::open(&path)?;
        let state = DatabaseState::create(&path)?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(Self {
            path,
            state: Some(state),
            active: None,
            journal,
            failed: false,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = std::path::absolute(path)?;
        let mut journal = Journal::open(&path)?;
        journal.recover()?;
        remove_staging(&path)?;
        let state = DatabaseState::open(&path)?;
        Ok(Self {
            path,
            state: Some(state),
            active: None,
            journal,
            failed: false,
        })
    }

    pub fn execute(&mut self, source: &str) -> Result<Vec<Output>, DatabaseError> {
        let statements = parse(source)?;
        statements
            .iter()
            .map(|statement| self.execute_statement(statement))
            .collect()
    }

    pub fn query(&mut self, source: &str) -> Result<Output, DatabaseError> {
        let statements = parse(source)?;
        if statements.len() != 1 || !matches!(statements[0], Statement::Select { .. }) {
            return Err(
                ExecutionError("query requires exactly one SELECT statement".into()).into(),
            );
        }
        self.execute_statement(&statements[0])
    }

    pub fn plan(&self, source: &str) -> Result<Vec<ScanPlan>, DatabaseError> {
        self.ensure_healthy()?;
        self.active
            .as_ref()
            .or(self.state.as_ref())
            .unwrap()
            .plan(source)
    }

    pub fn execute_statement(&mut self, statement: &Statement) -> Result<Output, DatabaseError> {
        self.ensure_healthy()?;
        let result = (|| match statement {
            Statement::Begin => {
                self.begin()?;
                Ok(Output::Begun)
            }
            Statement::Commit => {
                self.commit()?;
                Ok(Output::Committed)
            }
            Statement::Rollback => {
                self.rollback()?;
                Ok(Output::RolledBack)
            }
            Statement::Select { .. } => self
                .active
                .as_mut()
                .or(self.state.as_mut())
                .unwrap()
                .execute_statement(statement),
            _ => {
                let automatic = self.active.is_none();
                if automatic {
                    self.begin()?;
                }
                let result = self.active.as_mut().unwrap().execute_statement(statement);
                match result {
                    Ok(output) => {
                        if automatic {
                            self.commit()?;
                        }
                        Ok(output)
                    }
                    Err(error) => {
                        if automatic && !matches!(error, DatabaseError::Storage(_)) {
                            self.rollback()?;
                        }
                        Err(error)
                    }
                }
            }
        })();
        if matches!(result, Err(DatabaseError::Storage(_))) {
            self.failed = true;
        }
        result
    }

    pub fn sync(&mut self) -> Result<(), DatabaseError> {
        self.ensure_healthy()?;
        let result = self.active.as_mut().or(self.state.as_mut()).unwrap().sync();
        if matches!(result, Err(DatabaseError::Storage(_))) {
            self.failed = true;
        }
        result
    }

    fn ensure_healthy(&self) -> Result<(), DatabaseError> {
        if self.failed {
            Err(ExecutionError(
                "database handle failed; discard it and reopen to run recovery".into(),
            )
            .into())
        } else {
            Ok(())
        }
    }

    fn begin(&mut self) -> Result<(), DatabaseError> {
        if self.active.is_some() {
            return Err(ExecutionError("a transaction is already active".into()).into());
        }
        remove_staging(&self.path)?;
        let staged = self.path.join(".transaction");
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&staged)?;
        for (name, source) in managed_files(&self.path)? {
            fs::copy(source, staged.join(name))?;
        }
        self.active = Some(DatabaseState::open(staged)?);
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DatabaseError> {
        self.active
            .as_mut()
            .ok_or_else(|| ExecutionError("no active transaction".into()))?
            .sync()?;
        self.active.take();
        self.state.take();
        self.journal
            .write_transaction(self.path.join(".transaction"))?;
        self.journal.recover()?;
        self.state = Some(DatabaseState::open(&self.path)?);
        remove_staging(&self.path)?;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), DatabaseError> {
        self.active
            .take()
            .ok_or_else(|| ExecutionError("no active transaction".into()))?;
        remove_staging(&self.path)?;
        Ok(())
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        self.active.take();
        let _ = remove_staging(&self.path);
    }
}

fn remove_staging(directory: &Path) -> io::Result<()> {
    let staged = directory.join(".transaction");
    match fs::symlink_metadata(&staged) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(staged),
        Ok(_) => Err(io::Error::new(
            ErrorKind::InvalidData,
            "transaction staging path must be a directory",
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
