use std::collections::{BTreeMap, HashSet};
use std::fmt;

use crate::storage::Value;

use super::{ColumnDefinition, Comparison, DataType, Expr, Projection, Statement};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionError(pub String);

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ExecutionError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Created,
    Affected(usize),
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
}

#[derive(Default)]
pub struct Session {
    tables: BTreeMap<String, Table>,
}

struct Table {
    columns: Vec<ColumnDefinition>,
    rows: Vec<Vec<Value>>,
}

impl Session {
    pub fn execute(&mut self, statement: &Statement) -> Result<Output, ExecutionError> {
        match statement {
            Statement::CreateTable { name, columns } => {
                if self.tables.contains_key(name) {
                    return Err(ExecutionError(format!("table {name:?} already exists")));
                }
                let mut names = HashSet::new();
                if columns.is_empty()
                    || columns
                        .iter()
                        .any(|column| column.name.is_empty() || !names.insert(&column.name))
                {
                    return Err(ExecutionError(
                        "table requires nonempty, unique column names".into(),
                    ));
                }
                if columns.iter().filter(|column| column.primary_key).count() > 1 {
                    return Err(ExecutionError(
                        "only one PRIMARY KEY column is supported".into(),
                    ));
                }
                self.tables.insert(
                    name.clone(),
                    Table {
                        columns: columns.clone(),
                        rows: Vec::new(),
                    },
                );
                Ok(Output::Created)
            }
            Statement::Insert {
                table,
                columns,
                rows,
            } => {
                let table = self
                    .tables
                    .get_mut(table)
                    .ok_or_else(|| ExecutionError(format!("unknown table {table:?}")))?;
                let targets = match columns {
                    Some(columns) => columns
                        .iter()
                        .map(|name| column_index(&table.columns, name))
                        .collect::<Result<Vec<_>, _>>()?,
                    None => (0..table.columns.len()).collect(),
                };
                let mut seen = HashSet::new();
                if targets.is_empty() || targets.iter().any(|target| !seen.insert(target)) {
                    return Err(ExecutionError(
                        "INSERT columns must be nonempty and unique".into(),
                    ));
                }
                let mut candidate = table.rows.clone();
                for values in rows {
                    if values.len() != targets.len() {
                        return Err(ExecutionError(format!(
                            "expected {} values, found {}",
                            targets.len(),
                            values.len()
                        )));
                    }
                    let mut row = vec![Value::Null; table.columns.len()];
                    for (&target, value) in targets.iter().zip(values) {
                        row[target] = value.clone();
                    }
                    candidate.push(row);
                }
                validate_rows(&table.columns, &candidate)?;
                table.rows = candidate;
                Ok(Output::Affected(rows.len()))
            }
            Statement::Select {
                table,
                projection,
                filter,
            } => {
                let table = self
                    .tables
                    .get(table)
                    .ok_or_else(|| ExecutionError(format!("unknown table {table:?}")))?;
                let targets = match projection {
                    Projection::All => (0..table.columns.len()).collect::<Vec<_>>(),
                    Projection::Columns(columns) => columns
                        .iter()
                        .map(|name| column_index(&table.columns, name))
                        .collect::<Result<Vec<_>, _>>()?,
                };
                let rows = filtered_rows(table, filter.as_ref())?
                    .into_iter()
                    .map(|index| {
                        targets
                            .iter()
                            .map(|&target| table.rows[index][target].clone())
                            .collect()
                    })
                    .collect();
                let columns = targets
                    .iter()
                    .map(|&target| table.columns[target].name.clone())
                    .collect();
                Ok(Output::Rows { columns, rows })
            }
            Statement::Update {
                table,
                assignments,
                filter,
            } => {
                let table = self
                    .tables
                    .get_mut(table)
                    .ok_or_else(|| ExecutionError(format!("unknown table {table:?}")))?;
                let mut seen = HashSet::new();
                let mut targets = Vec::new();
                for (name, value) in assignments {
                    let target = column_index(&table.columns, name)?;
                    validate_value(&table.columns[target], value)?;
                    if !seen.insert(target) {
                        return Err(ExecutionError(format!("duplicate assignment to {name:?}")));
                    }
                    targets.push((target, value));
                }
                let selected = filtered_rows(table, filter.as_ref())?;
                let mut candidate = table.rows.clone();
                for &index in &selected {
                    for &(target, value) in &targets {
                        candidate[index][target] = value.clone();
                    }
                }
                validate_rows(&table.columns, &candidate)?;
                table.rows = candidate;
                Ok(Output::Affected(selected.len()))
            }
            Statement::Delete { table, filter } => {
                let table = self
                    .tables
                    .get_mut(table)
                    .ok_or_else(|| ExecutionError(format!("unknown table {table:?}")))?;
                let selected = filtered_rows(table, filter.as_ref())?;
                table.rows = table
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| selected.binary_search(index).is_err())
                    .map(|(_, row)| row.clone())
                    .collect();
                Ok(Output::Affected(selected.len()))
            }
        }
    }
}

fn column_index(columns: &[ColumnDefinition], name: &str) -> Result<usize, ExecutionError> {
    columns
        .iter()
        .position(|column| column.name == name)
        .ok_or_else(|| ExecutionError(format!("unknown column {name:?}")))
}

fn validate_rows(columns: &[ColumnDefinition], rows: &[Vec<Value>]) -> Result<(), ExecutionError> {
    for row in rows {
        if row.len() != columns.len() {
            return Err(ExecutionError("row width does not match schema".into()));
        }
        for (column, value) in columns.iter().zip(row) {
            validate_value(column, value)?;
        }
    }
    if let Some(primary) = columns.iter().position(|column| column.primary_key) {
        let mut integer_keys = HashSet::new();
        let mut text_keys = HashSet::new();
        for row in rows {
            let unique = match &row[primary] {
                Value::Integer(value) => integer_keys.insert(*value),
                Value::Text(value) => text_keys.insert(value.as_str()),
                Value::Null => false,
            };
            if !unique {
                return Err(ExecutionError("duplicate or NULL PRIMARY KEY".into()));
            }
        }
    }
    Ok(())
}

fn filtered_rows(table: &Table, filter: Option<&Expr>) -> Result<Vec<usize>, ExecutionError> {
    let Some(filter) = filter else {
        return Ok((0..table.rows.len()).collect());
    };
    if expression_type(filter, &table.columns)? == Some(DataType::Text) {
        return Err(ExecutionError(
            "boolean expressions require INTEGER or NULL".into(),
        ));
    }
    let mut selected = Vec::new();
    for (index, row) in table.rows.iter().enumerate() {
        if truth(evaluate(filter, &table.columns, row)?)? == Some(true) {
            selected.push(index);
        }
    }
    Ok(selected)
}

fn truth(value: Value) -> Result<Option<bool>, ExecutionError> {
    match value {
        Value::Null => Ok(None),
        Value::Integer(value) => Ok(Some(value != 0)),
        Value::Text(_) => Err(ExecutionError(
            "boolean expressions require INTEGER or NULL".into(),
        )),
    }
}

fn validate_value(column: &ColumnDefinition, value: &Value) -> Result<(), ExecutionError> {
    let valid = match (column.data_type, value) {
        (_, Value::Null) => !column.not_null && !column.primary_key,
        (DataType::Integer, Value::Integer(_)) | (DataType::Text, Value::Text(_)) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ExecutionError(format!(
            "value violates type or NULL constraint for column {:?}",
            column.name
        )))
    }
}

fn expression_type(
    expression: &Expr,
    columns: &[ColumnDefinition],
) -> Result<Option<DataType>, ExecutionError> {
    match expression {
        Expr::Literal(Value::Null) => Ok(None),
        Expr::Literal(Value::Integer(_)) => Ok(Some(DataType::Integer)),
        Expr::Literal(Value::Text(_)) => Ok(Some(DataType::Text)),
        Expr::Column(name) => Ok(Some(columns[column_index(columns, name)?].data_type)),
        Expr::Compare { left, right, .. } => {
            let left = expression_type(left, columns)?;
            let right = expression_type(right, columns)?;
            if left.is_some() && right.is_some() && left != right {
                return Err(ExecutionError(
                    "comparison requires matching INTEGER or TEXT types".into(),
                ));
            }
            Ok(Some(DataType::Integer))
        }
        Expr::IsNull { expression, .. } => {
            expression_type(expression, columns)?;
            Ok(Some(DataType::Integer))
        }
        Expr::Not(expression) => {
            if expression_type(expression, columns)? == Some(DataType::Text) {
                return Err(ExecutionError(
                    "boolean expressions require INTEGER or NULL".into(),
                ));
            }
            Ok(Some(DataType::Integer))
        }
        Expr::And(expressions) | Expr::Or(expressions) => {
            for expression in expressions {
                if expression_type(expression, columns)? == Some(DataType::Text) {
                    return Err(ExecutionError(
                        "boolean expressions require INTEGER or NULL".into(),
                    ));
                }
            }
            Ok(Some(DataType::Integer))
        }
    }
}

fn evaluate(
    expression: &Expr,
    columns: &[ColumnDefinition],
    row: &[Value],
) -> Result<Value, ExecutionError> {
    match expression {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Column(name) => Ok(row[column_index(columns, name)?].clone()),
        Expr::Compare {
            left,
            operator,
            right,
        } => {
            let left = evaluate(left, columns, row)?;
            let right = evaluate(right, columns, row)?;
            let ordering = match (&left, &right) {
                (Value::Null, _) | (_, Value::Null) => return Ok(Value::Null),
                (Value::Integer(left), Value::Integer(right)) => left.cmp(right),
                (Value::Text(left), Value::Text(right)) => left.cmp(right),
                _ => {
                    return Err(ExecutionError(
                        "comparison requires matching INTEGER or TEXT types".into(),
                    ));
                }
            };
            let result = match operator {
                Comparison::Equal => ordering.is_eq(),
                Comparison::NotEqual => !ordering.is_eq(),
                Comparison::Less => ordering.is_lt(),
                Comparison::LessEqual => !ordering.is_gt(),
                Comparison::Greater => ordering.is_gt(),
                Comparison::GreaterEqual => !ordering.is_lt(),
            };
            Ok(Value::Integer(i64::from(result)))
        }
        Expr::IsNull {
            expression,
            negated,
        } => Ok(Value::Integer(i64::from(
            matches!(evaluate(expression, columns, row)?, Value::Null) != *negated,
        ))),
        Expr::Not(expression) => Ok(truth(evaluate(expression, columns, row)?)?
            .map_or(Value::Null, |value| Value::Integer(i64::from(!value)))),
        Expr::And(expressions) | Expr::Or(expressions) => {
            let conjunction = matches!(expression, Expr::And(_));
            let mut result = Some(conjunction);
            for expression in expressions {
                let next = truth(evaluate(expression, columns, row)?)?;
                result = if conjunction {
                    match (result, next) {
                        (Some(false), _) | (_, Some(false)) => Some(false),
                        (Some(true), Some(true)) => Some(true),
                        _ => None,
                    }
                } else {
                    match (result, next) {
                        (Some(true), _) | (_, Some(true)) => Some(true),
                        (Some(false), Some(false)) => Some(false),
                        _ => None,
                    }
                };
            }
            Ok(result.map_or(Value::Null, |value| Value::Integer(i64::from(value))))
        }
    }
}
