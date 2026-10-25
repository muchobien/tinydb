use crate::storage::Value;

use super::{
    Aggregate, ColumnDefinition, ColumnRef, Comparison, DataType, Expr, Join, OrderBy, Projection,
    SelectExpr, SelectItem, SqlError, Statement, Token, TokenKind, tokenize,
};

pub fn parse(source: &str) -> Result<Vec<Statement>, SqlError> {
    let mut parser = Parser {
        source,
        tokens: tokenize(source)?,
        position: 0,
    };
    let mut statements = Vec::new();
    while parser.position < parser.tokens.len() {
        if parser.consume(&TokenKind::Semicolon) {
            continue;
        }
        statements.push(parser.statement()?);
        if !parser.consume(&TokenKind::Semicolon) && parser.position < parser.tokens.len() {
            return Err(parser.error("expected ';' between statements"));
        }
    }
    Ok(statements)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    position: usize,
}

impl Parser<'_> {
    fn error(&self, message: impl Into<String>) -> SqlError {
        let span = self
            .tokens
            .get(self.position)
            .map_or(self.source.len()..self.source.len(), |token| {
                token.span.clone()
            });
        SqlError::new(self.source, span, message)
    }

    fn consume(&mut self, expected: &TokenKind) -> bool {
        if self
            .tokens
            .get(self.position)
            .is_some_and(|token| &token.kind == expected)
        {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn keyword(&mut self, expected: &str) -> bool {
        if self.tokens.get(self.position).is_some_and(
            |token| matches!(&token.kind, TokenKind::Keyword(word) if word == expected),
        ) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: TokenKind) -> Result<(), SqlError> {
        if self.consume(&expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected {expected:?}")))
        }
    }

    fn expect_keyword(&mut self, expected: &str) -> Result<(), SqlError> {
        if self.keyword(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected {expected}")))
        }
    }

    fn identifier(&mut self) -> Result<String, SqlError> {
        if let Some(Token {
            kind: TokenKind::Identifier(name),
            ..
        }) = self.tokens.get(self.position)
        {
            let name = name.clone();
            self.position += 1;
            Ok(name)
        } else {
            Err(self.error("expected an identifier (quote reserved words with double quotes)"))
        }
    }

    fn identifiers(&mut self) -> Result<Vec<String>, SqlError> {
        let mut names = vec![self.identifier()?];
        while self.consume(&TokenKind::Comma) {
            names.push(self.identifier()?);
        }
        Ok(names)
    }

    fn column(&mut self) -> Result<ColumnRef, SqlError> {
        let first = self.identifier()?;
        if self.consume(&TokenKind::Dot) {
            Ok(ColumnRef {
                table: Some(first),
                name: self.identifier()?,
            })
        } else {
            Ok(ColumnRef {
                table: None,
                name: first,
            })
        }
    }

    fn alias(&mut self) -> Result<Option<String>, SqlError> {
        if self.keyword("AS") {
            Ok(Some(self.identifier()?))
        } else {
            Ok(None)
        }
    }

    fn literal(&mut self) -> Result<Value, SqlError> {
        if self.keyword("NULL") {
            return Ok(Value::Null);
        }
        if let Some(Token {
            kind: TokenKind::String(value),
            ..
        }) = self.tokens.get(self.position)
        {
            let value = Value::Text(value.clone());
            self.position += 1;
            return Ok(value);
        }
        let start = self.position;
        let negative = self.consume(&TokenKind::Minus);
        if !negative {
            self.consume(&TokenKind::Plus);
        }
        if let Some(Token {
            kind: TokenKind::Number(number),
            ..
        }) = self.tokens.get(self.position)
        {
            let text = if negative {
                format!("-{number}")
            } else {
                number.clone()
            };
            let value = text.parse::<i64>().map_err(|_| {
                let first = self.tokens[start].span.start;
                SqlError::new(
                    self.source,
                    first..self.tokens[self.position].span.end,
                    "integer literal is outside the i64 range",
                )
            })?;
            self.position += 1;
            return Ok(Value::Integer(value));
        }
        Err(self.error("expected an integer, string, or NULL literal"))
    }

    fn statement(&mut self) -> Result<Statement, SqlError> {
        if self.keyword("CREATE") {
            self.expect_keyword("TABLE")?;
            let name = self.identifier()?;
            self.expect(TokenKind::LeftParen)?;
            let mut columns = Vec::new();
            loop {
                let name = self.identifier()?;
                let data_type = if self.keyword("INTEGER") {
                    DataType::Integer
                } else if self.keyword("TEXT") {
                    DataType::Text
                } else {
                    return Err(self.error("expected INTEGER or TEXT"));
                };
                let mut not_null = false;
                let mut primary_key = false;
                loop {
                    if self.keyword("NOT") {
                        if not_null {
                            return Err(self.error("duplicate NOT NULL constraint"));
                        }
                        self.expect_keyword("NULL")?;
                        not_null = true;
                    } else if self.keyword("PRIMARY") {
                        if primary_key {
                            return Err(self.error("duplicate PRIMARY KEY constraint"));
                        }
                        self.expect_keyword("KEY")?;
                        primary_key = true;
                    } else {
                        break;
                    }
                }
                columns.push(ColumnDefinition {
                    name,
                    data_type,
                    not_null,
                    primary_key,
                });
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RightParen)?;
            return Ok(Statement::CreateTable { name, columns });
        }
        if self.keyword("INSERT") {
            self.expect_keyword("INTO")?;
            let table = self.identifier()?;
            let columns = if self.consume(&TokenKind::LeftParen) {
                let columns = self.identifiers()?;
                self.expect(TokenKind::RightParen)?;
                Some(columns)
            } else {
                None
            };
            self.expect_keyword("VALUES")?;
            let mut rows = Vec::new();
            loop {
                self.expect(TokenKind::LeftParen)?;
                let mut values = vec![self.literal()?];
                while self.consume(&TokenKind::Comma) {
                    values.push(self.literal()?);
                }
                self.expect(TokenKind::RightParen)?;
                rows.push(values);
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
            return Ok(Statement::Insert {
                table,
                columns,
                rows,
            });
        }
        if self.keyword("SELECT") {
            let projection = if self.consume(&TokenKind::Star) {
                Projection::All
            } else {
                let mut items = Vec::new();
                loop {
                    let function = if self.keyword("COUNT") {
                        Some(Aggregate::Count)
                    } else if self.keyword("SUM") {
                        Some(Aggregate::Sum)
                    } else if self.keyword("MIN") {
                        Some(Aggregate::Min)
                    } else if self.keyword("MAX") {
                        Some(Aggregate::Max)
                    } else {
                        None
                    };
                    let expression = if let Some(function) = function {
                        self.expect(TokenKind::LeftParen)?;
                        let column =
                            if function == Aggregate::Count && self.consume(&TokenKind::Star) {
                                None
                            } else {
                                Some(self.column()?)
                            };
                        self.expect(TokenKind::RightParen)?;
                        SelectExpr::Aggregate { function, column }
                    } else {
                        SelectExpr::Column(self.column()?)
                    };
                    items.push(SelectItem {
                        expression,
                        alias: self.alias()?,
                    });
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
                Projection::Items(items)
            };
            self.expect_keyword("FROM")?;
            let table = self.identifier()?;
            let alias = self.alias()?;
            let mut joins = Vec::new();
            loop {
                let inner = self.keyword("INNER");
                if !self.keyword("JOIN") {
                    if inner {
                        return Err(self.error("expected JOIN after INNER"));
                    }
                    break;
                }
                let table = self.identifier()?;
                let alias = self.alias()?;
                self.expect_keyword("ON")?;
                joins.push(Join {
                    table,
                    alias,
                    on: self.expression(0)?,
                });
            }
            let filter = self.filter()?;
            let mut group_by = Vec::new();
            if self.keyword("GROUP") {
                self.expect_keyword("BY")?;
                loop {
                    group_by.push(self.column()?);
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            let mut order_by = Vec::new();
            if self.keyword("ORDER") {
                self.expect_keyword("BY")?;
                loop {
                    let column = self.column()?;
                    let descending = self.keyword("DESC");
                    if !descending {
                        self.keyword("ASC");
                    }
                    order_by.push(OrderBy { column, descending });
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            return Ok(Statement::Select {
                table,
                alias,
                joins,
                projection,
                filter,
                group_by,
                order_by,
            });
        }
        if self.keyword("UPDATE") {
            let table = self.identifier()?;
            self.expect_keyword("SET")?;
            let mut assignments = Vec::new();
            loop {
                let column = self.identifier()?;
                self.expect(TokenKind::Equal)?;
                assignments.push((column, self.literal()?));
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
            return Ok(Statement::Update {
                table,
                assignments,
                filter: self.filter()?,
            });
        }
        if self.keyword("DELETE") {
            self.expect_keyword("FROM")?;
            let table = self.identifier()?;
            return Ok(Statement::Delete {
                table,
                filter: self.filter()?,
            });
        }
        Err(self.error("expected CREATE TABLE, INSERT, SELECT, UPDATE, or DELETE"))
    }

    fn filter(&mut self) -> Result<Option<Expr>, SqlError> {
        if self.keyword("WHERE") {
            Ok(Some(self.expression(0)?))
        } else {
            Ok(None)
        }
    }

    fn expression(&mut self, depth: usize) -> Result<Expr, SqlError> {
        let mut expressions = vec![self.conjunction(depth)?];
        while self.keyword("OR") {
            expressions.push(self.conjunction(depth)?);
        }
        if expressions.len() == 1 {
            Ok(expressions.pop().unwrap())
        } else {
            Ok(Expr::Or(expressions))
        }
    }

    fn conjunction(&mut self, depth: usize) -> Result<Expr, SqlError> {
        let mut expressions = vec![self.negation(depth)?];
        while self.keyword("AND") {
            expressions.push(self.negation(depth)?);
        }
        if expressions.len() == 1 {
            Ok(expressions.pop().unwrap())
        } else {
            Ok(Expr::And(expressions))
        }
    }

    fn negation(&mut self, depth: usize) -> Result<Expr, SqlError> {
        if depth >= 64 {
            return Err(self.error("expression nesting exceeds 64 levels"));
        }
        if self.keyword("NOT") {
            return Ok(Expr::Not(Box::new(self.negation(depth + 1)?)));
        }
        let left = self.primary(depth)?;
        if self.keyword("IS") {
            let negated = self.keyword("NOT");
            self.expect_keyword("NULL")?;
            return Ok(Expr::IsNull {
                expression: Box::new(left),
                negated,
            });
        }
        let operator = match self.tokens.get(self.position).map(|token| &token.kind) {
            Some(TokenKind::Equal) => Comparison::Equal,
            Some(TokenKind::NotEqual) => Comparison::NotEqual,
            Some(TokenKind::Less) => Comparison::Less,
            Some(TokenKind::LessEqual) => Comparison::LessEqual,
            Some(TokenKind::Greater) => Comparison::Greater,
            Some(TokenKind::GreaterEqual) => Comparison::GreaterEqual,
            _ => return Ok(left),
        };
        self.position += 1;
        Ok(Expr::Compare {
            left: Box::new(left),
            operator,
            right: Box::new(self.primary(depth)?),
        })
    }

    fn primary(&mut self, depth: usize) -> Result<Expr, SqlError> {
        if self.consume(&TokenKind::LeftParen) {
            let expression = self.expression(depth + 1)?;
            self.expect(TokenKind::RightParen)?;
            Ok(expression)
        } else if matches!(
            self.tokens.get(self.position).map(|token| &token.kind),
            Some(TokenKind::Identifier(_))
        ) {
            Ok(Expr::Column(self.column()?))
        } else {
            Ok(Expr::Literal(self.literal()?))
        }
    }
}
