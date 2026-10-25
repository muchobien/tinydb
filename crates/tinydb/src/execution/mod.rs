mod database;
mod planner;
mod query;

pub use database::{Database, DatabaseError};
pub use planner::{AccessPath, ScanPlan};

use crate::sql::DataType;

pub(crate) struct Field {
    pub name: String,
    pub table: String,
    pub data_type: DataType,
}

pub(crate) use query::select;
