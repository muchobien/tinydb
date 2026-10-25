use crate::storage::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<ColumnDefinition>,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Value>>,
    },
    Select {
        table: String,
        alias: Option<String>,
        joins: Vec<Join>,
        projection: Projection,
        filter: Option<Expr>,
        group_by: Vec<ColumnRef>,
        order_by: Vec<OrderBy>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Value)>,
        filter: Option<Expr>,
    },
    Delete {
        table: String,
        filter: Option<Expr>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: DataType,
    pub not_null: bool,
    pub primary_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataType {
    Integer,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Projection {
    All,
    Items(Vec<SelectItem>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnRef {
    pub table: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Join {
    pub table: String,
    pub alias: Option<String>,
    pub on: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectItem {
    pub expression: SelectExpr,
    pub alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectExpr {
    Column(ColumnRef),
    Aggregate {
        function: Aggregate,
        column: Option<ColumnRef>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    Count,
    Sum,
    Min,
    Max,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderBy {
    pub column: ColumnRef,
    pub descending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Literal(Value),
    Column(ColumnRef),
    Compare {
        left: Box<Expr>,
        operator: Comparison,
        right: Box<Expr>,
    },
    IsNull {
        expression: Box<Expr>,
        negated: bool,
    },
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
