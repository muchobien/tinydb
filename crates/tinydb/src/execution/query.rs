use std::collections::{BTreeMap, HashSet};

use crate::sql::{
    Aggregate, DataType, ExecutionError, Output, Projection, SelectExpr, Statement, Table,
    evaluate, resolve, truth, validate_predicate,
};
use crate::storage::Value;

use super::Field;

pub(crate) fn select(
    tables: &BTreeMap<String, Table>,
    statement: &Statement,
) -> Result<Output, ExecutionError> {
    let Statement::Select {
        table,
        alias,
        joins,
        projection,
        filter,
        group_by,
        order_by,
    } = statement
    else {
        return Err(ExecutionError("expected SELECT".into()));
    };
    let source = tables
        .get(table)
        .ok_or_else(|| ExecutionError(format!("unknown table {table:?}")))?;
    let qualifier = alias.as_ref().unwrap_or(table);
    let mut qualifiers = HashSet::from([qualifier.as_str()]);
    let mut fields: Vec<_> = source
        .columns
        .iter()
        .map(|column| Field {
            name: column.name.clone(),
            table: qualifier.clone(),
            data_type: column.data_type,
        })
        .collect();
    let mut input = source.rows.clone();
    for join in joins {
        let source = tables
            .get(&join.table)
            .ok_or_else(|| ExecutionError(format!("unknown table {:?}", join.table)))?;
        let qualifier = join.alias.as_ref().unwrap_or(&join.table);
        if !qualifiers.insert(qualifier.as_str()) {
            return Err(ExecutionError(format!(
                "duplicate table qualifier {qualifier:?}"
            )));
        }
        fields.extend(source.columns.iter().map(|column| Field {
            name: column.name.clone(),
            table: qualifier.clone(),
            data_type: column.data_type,
        }));
        validate_predicate(&join.on, &fields)?;
        let mut joined = Vec::new();
        for left in &input {
            for right in &source.rows {
                let mut row = left.clone();
                row.extend_from_slice(right);
                if truth(evaluate(&join.on, &fields, &row)?)? == Some(true) {
                    joined.push(row);
                }
            }
        }
        input = joined;
    }
    if let Some(filter) = filter {
        validate_predicate(filter, &fields)?;
        let mut filtered = Vec::new();
        for row in input {
            if truth(evaluate(filter, &fields, &row)?)? == Some(true) {
                filtered.push(row);
            }
        }
        input = filtered;
    }
    let group_columns = group_by
        .iter()
        .map(|column| resolve(&fields, column))
        .collect::<Result<Vec<_>, _>>()?;
    let mut bindings = Vec::new();
    let mut labels = Vec::new();
    match projection {
        Projection::All => {
            for (index, field) in fields.iter().enumerate() {
                bindings.push((None, Some(index)));
                labels.push(if joins.is_empty() {
                    field.name.clone()
                } else {
                    format!("{}.{}", field.table, field.name)
                });
            }
        }
        Projection::Items(items) => {
            for item in items {
                let (binding, label) = match &item.expression {
                    SelectExpr::Column(column) => (
                        (None, Some(resolve(&fields, column)?)),
                        column.table.as_ref().map_or_else(
                            || column.name.clone(),
                            |table| format!("{table}.{}", column.name),
                        ),
                    ),
                    SelectExpr::Aggregate { function, column } => {
                        let index = column
                            .as_ref()
                            .map(|column| resolve(&fields, column))
                            .transpose()?;
                        if index.is_none() && *function != Aggregate::Count {
                            return Err(ExecutionError("only COUNT accepts '*'".into()));
                        }
                        if *function == Aggregate::Sum
                            && index
                                .is_some_and(|index| fields[index].data_type != DataType::Integer)
                        {
                            return Err(ExecutionError("SUM requires an INTEGER column".into()));
                        }
                        let name = column.as_ref().map_or_else(
                            || "*".into(),
                            |column| {
                                column.table.as_ref().map_or_else(
                                    || column.name.clone(),
                                    |table| format!("{table}.{}", column.name),
                                )
                            },
                        );
                        (
                            (Some(*function), index),
                            format!("{}({name})", format!("{function:?}").to_ascii_uppercase()),
                        )
                    }
                };
                bindings.push(binding);
                labels.push(item.alias.clone().unwrap_or(label));
            }
        }
    }
    let grouped = !group_by.is_empty() || bindings.iter().any(|(function, _)| function.is_some());
    if grouped
        && bindings
            .iter()
            .any(|(function, index)| function.is_none() && !group_columns.contains(&index.unwrap()))
    {
        return Err(ExecutionError(
            "non-aggregate projection columns must appear in GROUP BY".into(),
        ));
    }
    let mut ordering = Vec::new();
    for order in order_by {
        let matches: Vec<_> = labels
            .iter()
            .enumerate()
            .filter(|(_, label)| {
                order.column.table.is_none() && label.eq_ignore_ascii_case(&order.column.name)
            })
            .map(|(index, _)| index)
            .collect();
        if matches.len() > 1 {
            return Err(ExecutionError(format!(
                "ambiguous output name {:?}",
                order.column.name
            )));
        }
        if let Some(&index) = matches.first() {
            ordering.push((index, true));
        } else {
            let index = resolve(&fields, &order.column)?;
            if grouped && !group_columns.contains(&index) {
                return Err(ExecutionError(
                    "ORDER BY column must appear in GROUP BY or reference an output alias".into(),
                ));
            }
            ordering.push((index, false));
        }
    }
    let groups: Vec<Vec<&[Value]>> = if grouped {
        let mut groups: BTreeMap<Vec<Value>, Vec<&[Value]>> = BTreeMap::new();
        if group_by.is_empty() {
            groups.insert(Vec::new(), Vec::new());
        }
        for row in &input {
            let key = group_columns
                .iter()
                .map(|&index| row[index].clone())
                .collect();
            groups.entry(key).or_default().push(row);
        }
        groups.into_values().collect()
    } else {
        input.iter().map(|row| vec![row.as_slice()]).collect()
    };
    let mut output = Vec::new();
    for group in groups {
        let mut values = Vec::new();
        for &(function, index) in &bindings {
            let value = match function {
                None => group[0][index.unwrap()].clone(),
                Some(Aggregate::Count) => {
                    let count = group
                        .iter()
                        .filter(|row| index.is_none_or(|index| row[index] != Value::Null))
                        .count();
                    Value::Integer(
                        i64::try_from(count)
                            .map_err(|_| ExecutionError("COUNT overflow".into()))?,
                    )
                }
                Some(Aggregate::Sum) => {
                    let mut total: Option<i64> = None;
                    for row in &group {
                        if let Value::Integer(value) = row[index.unwrap()] {
                            total = Some(
                                total
                                    .unwrap_or(0)
                                    .checked_add(value)
                                    .ok_or_else(|| ExecutionError("SUM overflow".into()))?,
                            );
                        }
                    }
                    total.map_or(Value::Null, Value::Integer)
                }
                Some(function) => {
                    let values = group
                        .iter()
                        .map(|row| &row[index.unwrap()])
                        .filter(|value| **value != Value::Null);
                    let value = if function == Aggregate::Min {
                        values.min()
                    } else {
                        values.max()
                    };
                    value.cloned().unwrap_or(Value::Null)
                }
            };
            values.push(value);
        }
        let keys = ordering
            .iter()
            .map(|&(index, from_output)| {
                if from_output {
                    values[index].clone()
                } else {
                    group[0][index].clone()
                }
            })
            .collect::<Vec<_>>();
        output.push((values, keys));
    }
    output.sort_by(|(_, left), (_, right)| {
        for (index, order) in order_by.iter().enumerate() {
            let comparison = left[index].cmp(&right[index]);
            let comparison = if order.descending {
                comparison.reverse()
            } else {
                comparison
            };
            if !comparison.is_eq() {
                return comparison;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(Output::Rows {
        columns: labels,
        rows: output.into_iter().map(|(row, _)| row).collect(),
    })
}
