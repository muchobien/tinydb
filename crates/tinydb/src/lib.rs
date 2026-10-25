//! An educational embedded database, starting with fixed-size disk pages.

pub mod execution;
pub mod index;
pub mod sql;
pub mod storage;

pub use execution::{Database, DatabaseError};
