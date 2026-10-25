use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use crate::index::BPlusTree;
use crate::sql::{
    ColumnDefinition, DataType, ExecutionError, Output, Session, SqlError, Statement, Table, parse,
    validate_rows,
};
use crate::storage::{MAX_RECORD_SIZE, Record, RecordStore, Value};

use super::{AccessPath, ScanPlan, planner};

#[derive(Debug)]
pub enum DatabaseError {
    Sql(SqlError),
    Execution(ExecutionError),
    Storage(io::Error),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(error) => error.fmt(formatter),
            Self::Execution(error) => error.fmt(formatter),
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sql(error) => Some(error),
            Self::Execution(error) => Some(error),
            Self::Storage(error) => Some(error),
        }
    }
}

impl From<io::Error> for DatabaseError {
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<ExecutionError> for DatabaseError {
    fn from(error: ExecutionError) -> Self {
        Self::Execution(error)
    }
}

impl From<SqlError> for DatabaseError {
    fn from(error: SqlError) -> Self {
        Self::Sql(error)
    }
}

pub struct Database {
    path: PathBuf,
    catalog: RecordStore,
    tables: BTreeMap<String, StoredTable>,
    next_id: i64,
    failed: bool,
}

struct StoredTable {
    columns: Vec<ColumnDefinition>,
    heap: RecordStore,
    index: Option<BPlusTree>,
    primary: Option<usize>,
}

impl Database {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = std::path::absolute(path)?;
        fs::create_dir(&path)?;
        let catalog = RecordStore::create(path.join("catalog.db"))?;
        let mut database = Self {
            path,
            catalog,
            tables: BTreeMap::new(),
            next_id: 1,
            failed: false,
        };
        database.sync()?;
        Ok(database)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = std::path::absolute(path)?;
        let mut catalog = RecordStore::open(path.join("catalog.db"))?;
        let mut tables = BTreeMap::new();
        let mut ids = HashSet::new();
        let mut next_id = 1;
        let mut validation = Session::default();
        for (_, record) in catalog.scan()? {
            let values = record.values;
            if values.len() < 7 || (values.len() - 3) % 4 != 0 || values[0] != Value::Integer(1) {
                return Err(invalid_data("invalid catalog record format"));
            }
            let Value::Integer(id) = values[1] else {
                return Err(invalid_data("invalid catalog table ID"));
            };
            if id <= 0 || !ids.insert(id) {
                return Err(invalid_data("duplicate or invalid catalog table ID"));
            }
            next_id = next_id.max(
                id.checked_add(1)
                    .ok_or_else(|| invalid_data("catalog table IDs exhausted"))?,
            );
            let Value::Text(name) = &values[2] else {
                return Err(invalid_data("invalid catalog table name"));
            };
            if name.is_empty() {
                return Err(invalid_data("empty catalog table name"));
            }
            let mut columns = Vec::new();
            for fields in values[3..].as_chunks::<4>().0 {
                let [
                    Value::Text(name),
                    Value::Integer(kind),
                    Value::Integer(not_null),
                    Value::Integer(primary_key),
                ] = fields
                else {
                    return Err(invalid_data("invalid catalog column definition"));
                };
                let data_type = match kind {
                    0 => DataType::Integer,
                    1 => DataType::Text,
                    _ => return Err(invalid_data("unknown catalog column type")),
                };
                if !matches!(not_null, 0 | 1) || !matches!(primary_key, 0 | 1) {
                    return Err(invalid_data("invalid catalog constraint flags"));
                }
                columns.push(ColumnDefinition {
                    name: name.clone(),
                    data_type,
                    not_null: *not_null == 1,
                    primary_key: *primary_key == 1,
                });
            }
            validation
                .execute(&Statement::CreateTable {
                    name: name.clone(),
                    columns: columns.clone(),
                })
                .map_err(|error| invalid_data(error.to_string()))?;
            let mut heap = RecordStore::open(path.join(format!("table-{id}.db")))?;
            let records = heap.scan()?;
            let rows: Vec<_> = records
                .iter()
                .map(|(_, record)| record.values.clone())
                .collect();
            validate_rows(&columns, &rows).map_err(|error| invalid_data(error.to_string()))?;
            let primary = columns
                .iter()
                .position(|column| column.primary_key && column.data_type == DataType::Integer);
            let index = if let Some(primary) = primary {
                let mut index = BPlusTree::open(path.join(format!("index-{id}.db")))?;
                let mut expected = Vec::new();
                for (id, record) in &records {
                    let Value::Integer(key) = record.values[primary] else {
                        return Err(invalid_data("non-integer index key"));
                    };
                    expected.push((key, *id));
                }
                expected.sort_by_key(|&(key, _)| key);
                if index.range(..)? != expected {
                    return Err(invalid_data(
                        "primary-key index does not match heap records",
                    ));
                }
                Some(index)
            } else {
                None
            };
            tables.insert(
                name.clone(),
                StoredTable {
                    columns,
                    heap,
                    index,
                    primary,
                },
            );
        }
        Ok(Self {
            path,
            catalog,
            tables,
            next_id,
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
        let statements = parse(source)?;
        if statements.len() != 1 || !matches!(statements[0], Statement::Select { .. }) {
            return Err(ExecutionError("plan requires exactly one SELECT statement".into()).into());
        }
        self.schema_session().execute(&statements[0])?;
        let schemas = self
            .tables
            .iter()
            .map(|(name, table)| (name.clone(), table.columns.clone()))
            .collect();
        Ok(planner::scans(&schemas, &statements[0])?)
    }

    pub fn sync(&mut self) -> Result<(), DatabaseError> {
        self.ensure_healthy()?;
        let result = (|| {
            self.catalog.sync()?;
            for table in self.tables.values() {
                table.heap.sync()?;
                if let Some(index) = &table.index {
                    index.sync()?;
                }
            }
            File::open(&self.path)?.sync_all()?;
            Ok::<_, io::Error>(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        Ok(result?)
    }

    fn ensure_healthy(&self) -> Result<(), DatabaseError> {
        if self.failed {
            Err(ExecutionError(
                "database handle failed; discard it and inspect the files before reopening".into(),
            )
            .into())
        } else {
            Ok(())
        }
    }

    fn schema_session(&self) -> Session {
        Session {
            tables: self
                .tables
                .iter()
                .map(|(name, table)| {
                    (
                        name.clone(),
                        Table {
                            columns: table.columns.clone(),
                            rows: Vec::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn execute_statement(&mut self, statement: &Statement) -> Result<Output, DatabaseError> {
        self.ensure_healthy()?;
        let result = (|| {
            let mut session = self.schema_session();
            if let Statement::CreateTable { name, columns } = statement {
                let output = session.execute(statement)?;
                let id = self.next_id;
                let next_id = id
                    .checked_add(1)
                    .ok_or_else(|| ExecutionError("catalog table IDs exhausted".into()))?;
                let mut values = vec![
                    Value::Integer(1),
                    Value::Integer(id),
                    Value::Text(name.clone()),
                ];
                for column in columns {
                    values.extend([
                        Value::Text(column.name.clone()),
                        Value::Integer(match column.data_type {
                            DataType::Integer => 0,
                            DataType::Text => 1,
                        }),
                        Value::Integer(i64::from(column.not_null)),
                        Value::Integer(i64::from(column.primary_key)),
                    ]);
                }
                let record = Record::new(values);
                if record
                    .encode()
                    .map_err(|error| ExecutionError(error.to_string()))?
                    .len()
                    > MAX_RECORD_SIZE
                {
                    return Err(ExecutionError(
                        "table schema exceeds catalog record capacity".into(),
                    )
                    .into());
                }
                let heap = RecordStore::create(self.path.join(format!("table-{id}.db")))?;
                let primary = columns
                    .iter()
                    .position(|column| column.primary_key && column.data_type == DataType::Integer);
                let index = if primary.is_some() {
                    Some(BPlusTree::create(self.path.join(format!("index-{id}.db")))?)
                } else {
                    None
                };
                heap.sync()?;
                if let Some(index) = &index {
                    index.sync()?;
                }
                self.catalog.insert(&record)?;
                self.tables.insert(
                    name.clone(),
                    StoredTable {
                        columns: columns.clone(),
                        heap,
                        index,
                        primary,
                    },
                );
                self.next_id = next_id;
                self.sync()?;
                return Ok(output);
            }
            if matches!(statement, Statement::Select { .. }) {
                session.execute(statement)?;
                let schemas = self
                    .tables
                    .iter()
                    .map(|(name, table)| (name.clone(), table.columns.clone()))
                    .collect();
                for plan in planner::scans(&schemas, statement)? {
                    let table = self.tables.get_mut(&plan.table).unwrap();
                    let records = match plan.access {
                        AccessPath::Sequential => table.heap.scan()?,
                        AccessPath::PrimaryKey { lower, upper } => {
                            let mut records = Vec::new();
                            for (_, id) in table.index.as_mut().unwrap().range((lower, upper))? {
                                records.push((id, table.heap.read(id)?));
                            }
                            records
                        }
                    };
                    session.tables.get_mut(&plan.table).unwrap().rows = records
                        .into_iter()
                        .map(|(_, record)| record.values)
                        .collect();
                }
                return Ok(session.execute(statement)?);
            }
            let name = match statement {
                Statement::Insert { table, .. }
                | Statement::Update { table, .. }
                | Statement::Delete { table, .. } => table,
                _ => unreachable!(),
            };
            let table = self
                .tables
                .get_mut(name)
                .ok_or_else(|| ExecutionError(format!("unknown table {name:?}")))?;
            let before = table.heap.scan()?;
            session.tables.get_mut(name).unwrap().rows = before
                .iter()
                .map(|(_, record)| record.values.clone())
                .collect();
            let output = session.execute(statement)?;
            let after = &session.tables[name].rows;
            let mut removed = Vec::new();
            let mut added = Vec::new();
            match statement {
                Statement::Insert { .. } => added.extend(after[before.len()..].iter().cloned()),
                Statement::Update { .. } => {
                    for ((id, record), row) in before.iter().zip(after) {
                        if &record.values != row {
                            removed.push((*id, &record.values));
                            added.push(row.clone());
                        }
                    }
                }
                Statement::Delete { .. } => {
                    let mut retained = after.iter().peekable();
                    for (id, record) in &before {
                        if retained.peek().is_some_and(|row| *row == &record.values) {
                            retained.next();
                        } else {
                            removed.push((*id, &record.values));
                        }
                    }
                }
                _ => unreachable!(),
            }
            let records: Vec<_> = added.into_iter().map(Record::new).collect();
            for record in &records {
                if record
                    .encode()
                    .map_err(|error| ExecutionError(error.to_string()))?
                    .len()
                    > MAX_RECORD_SIZE
                {
                    return Err(ExecutionError(
                        "encoded row exceeds single-page record capacity".into(),
                    )
                    .into());
                }
            }
            if let (Some(primary), Some(index)) = (table.primary, &mut table.index) {
                for (_, row) in &removed {
                    if let Value::Integer(key) = row[primary] {
                        index.delete(key)?;
                    }
                }
            }
            for record in &records {
                let id = table.heap.insert(record)?;
                if let (Some(primary), Some(index)) = (table.primary, &mut table.index)
                    && let Value::Integer(key) = record.values[primary]
                {
                    index.insert(key, id)?;
                }
            }
            for (id, _) in removed {
                table.heap.delete(id)?;
            }
            self.sync()?;
            Ok(output)
        })();
        if matches!(result, Err(DatabaseError::Storage(_))) {
            self.failed = true;
        }
        result
    }
}

fn invalid_data(message: impl Into<String>) -> DatabaseError {
    DatabaseError::Storage(io::Error::new(ErrorKind::InvalidData, message.into()))
}
