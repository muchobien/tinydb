mod ast;
mod lexer;
mod parser;
mod session;

use std::fmt;
use std::ops::Range;

pub use ast::{ColumnDefinition, Comparison, DataType, Expr, Projection, Statement};
pub use lexer::{Token, TokenKind, tokenize};
pub use parser::parse;
pub use session::{ExecutionError, Output, Session};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlError {
    pub message: String,
    pub span: Range<usize>,
    pub line: usize,
    pub column: usize,
}

impl SqlError {
    fn new(source: &str, span: Range<usize>, message: impl Into<String>) -> Self {
        let prefix = &source[..span.start];
        let line = prefix
            .chars()
            .filter(|&character| character == '\n')
            .count()
            + 1;
        let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
        Self {
            message: message.into(),
            span,
            line,
            column,
        }
    }
}

impl fmt::Display for SqlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at line {}, column {}",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for SqlError {}
