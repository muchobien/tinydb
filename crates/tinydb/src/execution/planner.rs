use std::collections::BTreeMap;
use std::ops::Bound;

use crate::sql::{ColumnDefinition, Comparison, DataType, ExecutionError, Expr, Statement};
use crate::storage::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessPath {
    Sequential,
    PrimaryKey {
        lower: Bound<i64>,
        upper: Bound<i64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanPlan {
    pub table: String,
    pub qualifier: String,
    pub access: AccessPath,
}

pub(crate) fn scans(
    schemas: &BTreeMap<String, Vec<ColumnDefinition>>,
    statement: &Statement,
) -> Result<Vec<ScanPlan>, ExecutionError> {
    let Statement::Select {
        table,
        alias,
        joins,
        filter,
        ..
    } = statement
    else {
        return Err(ExecutionError("scan plans require SELECT".into()));
    };
    let mut sources = vec![(table, alias.as_ref().unwrap_or(table))];
    sources.extend(
        joins
            .iter()
            .map(|join| (&join.table, join.alias.as_ref().unwrap_or(&join.table))),
    );
    let mut plans = Vec::new();
    for (table, qualifier) in sources {
        let columns = schemas
            .get(table)
            .ok_or_else(|| ExecutionError(format!("unknown table {table:?}")))?;
        let mut access = AccessPath::Sequential;
        if joins.is_empty()
            && let Some(primary) = columns
                .iter()
                .find(|column| column.primary_key && column.data_type == DataType::Integer)
            && let Some(filter) = filter
        {
            let mut lower = Bound::Unbounded;
            let mut upper = Bound::Unbounded;
            if constrain(filter, qualifier, &primary.name, &mut lower, &mut upper) {
                access = AccessPath::PrimaryKey { lower, upper };
            }
        }
        plans.push(ScanPlan {
            table: table.clone(),
            qualifier: qualifier.clone(),
            access,
        });
    }
    Ok(plans)
}

fn constrain(
    expression: &Expr,
    table: &str,
    primary: &str,
    lower: &mut Bound<i64>,
    upper: &mut Bound<i64>,
) -> bool {
    if let Expr::And(expressions) = expression {
        let mut indexed = false;
        for expression in expressions {
            indexed |= constrain(expression, table, primary, lower, upper);
        }
        return indexed;
    }
    let Expr::Compare {
        left,
        operator,
        right,
    } = expression
    else {
        return false;
    };
    let (column, key, operator) = match (left.as_ref(), right.as_ref()) {
        (Expr::Column(column), Expr::Literal(Value::Integer(key))) => (column, *key, *operator),
        (Expr::Literal(Value::Integer(key)), Expr::Column(column)) => {
            let reversed = match operator {
                Comparison::Equal => Comparison::Equal,
                Comparison::NotEqual => Comparison::NotEqual,
                Comparison::Less => Comparison::Greater,
                Comparison::LessEqual => Comparison::GreaterEqual,
                Comparison::Greater => Comparison::Less,
                Comparison::GreaterEqual => Comparison::LessEqual,
            };
            (column, *key, reversed)
        }
        _ => return false,
    };
    if column.name != primary
        || column
            .table
            .as_ref()
            .is_some_and(|qualifier| qualifier != table)
    {
        return false;
    }
    match operator {
        Comparison::Equal => {
            tighten(lower, Bound::Included(key), true);
            tighten(upper, Bound::Included(key), false);
        }
        Comparison::Less => tighten(upper, Bound::Excluded(key), false),
        Comparison::LessEqual => tighten(upper, Bound::Included(key), false),
        Comparison::Greater => tighten(lower, Bound::Excluded(key), true),
        Comparison::GreaterEqual => tighten(lower, Bound::Included(key), true),
        Comparison::NotEqual => return false,
    }
    true
}

fn tighten(current: &mut Bound<i64>, candidate: Bound<i64>, lower: bool) {
    let (key, excluded) = match candidate {
        Bound::Included(key) => (key, false),
        Bound::Excluded(key) => (key, true),
        Bound::Unbounded => return,
    };
    let replace = match *current {
        Bound::Unbounded => true,
        Bound::Included(old) | Bound::Excluded(old) => {
            let stricter = if lower { key > old } else { key < old };
            stricter || key == old && excluded
        }
    };
    if replace {
        *current = candidate;
    }
}
