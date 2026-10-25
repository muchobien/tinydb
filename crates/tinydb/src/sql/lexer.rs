use std::ops::Range;

use super::SqlError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Keyword(String),
    Identifier(String),
    Number(String),
    String(String),
    LeftParen,
    RightParen,
    Comma,
    Dot,
    Semicolon,
    Star,
    Plus,
    Minus,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

pub fn tokenize(source: &str) -> Result<Vec<Token>, SqlError> {
    let mut characters = source.char_indices().peekable();
    let mut tokens = Vec::new();
    while let Some((start, character)) = characters.next() {
        if character.is_whitespace() {
            continue;
        }
        if character == '-' && characters.peek().is_some_and(|&(_, next)| next == '-') {
            for (_, next) in characters.by_ref() {
                if next == '\n' {
                    break;
                }
            }
            continue;
        }
        if character == '/' && characters.peek().is_some_and(|&(_, next)| next == '*') {
            characters.next();
            let mut closed = false;
            while let Some((_, next)) = characters.next() {
                if next == '*' && characters.peek().is_some_and(|&(_, next)| next == '/') {
                    characters.next();
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err(SqlError::new(
                    source,
                    start..source.len(),
                    "unterminated block comment",
                ));
            }
            continue;
        }
        let kind = match character {
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            ';' => TokenKind::Semicolon,
            '*' => TokenKind::Star,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '=' => TokenKind::Equal,
            '<' | '>' | '!' => {
                let following = characters.peek().map(|&(_, next)| next);
                match (character, following) {
                    ('<', Some('=')) => {
                        characters.next();
                        TokenKind::LessEqual
                    }
                    ('>', Some('=')) => {
                        characters.next();
                        TokenKind::GreaterEqual
                    }
                    ('!', Some('=')) | ('<', Some('>')) => {
                        characters.next();
                        TokenKind::NotEqual
                    }
                    ('<', _) => TokenKind::Less,
                    ('>', _) => TokenKind::Greater,
                    _ => {
                        return Err(SqlError::new(
                            source,
                            start..start + 1,
                            "expected '=' after '!'",
                        ));
                    }
                }
            }
            '\'' | '"' => {
                let mut value = String::new();
                let mut closed = false;
                while let Some((_, next)) = characters.next() {
                    if next == character {
                        if characters
                            .peek()
                            .is_some_and(|&(_, next)| next == character)
                        {
                            characters.next();
                            value.push(character);
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        value.push(next);
                    }
                }
                if !closed {
                    return Err(SqlError::new(
                        source,
                        start..source.len(),
                        "unterminated quoted value",
                    ));
                }
                if character == '\'' {
                    TokenKind::String(value)
                } else {
                    if value.is_empty() {
                        return Err(SqlError::new(
                            source,
                            start..start + 2,
                            "identifier cannot be empty",
                        ));
                    }
                    TokenKind::Identifier(value.to_ascii_lowercase())
                }
            }
            character if character.is_ascii_digit() => {
                while characters
                    .peek()
                    .is_some_and(|&(_, next)| next.is_ascii_digit())
                {
                    characters.next();
                }
                let end = characters
                    .peek()
                    .map_or(source.len(), |&(offset, _)| offset);
                TokenKind::Number(source[start..end].to_owned())
            }
            character if character.is_alphabetic() || character == '_' => {
                while characters
                    .peek()
                    .is_some_and(|&(_, next)| next.is_alphanumeric() || next == '_')
                {
                    characters.next();
                }
                let end = characters
                    .peek()
                    .map_or(source.len(), |&(offset, _)| offset);
                let word = &source[start..end];
                let uppercase = word.to_ascii_uppercase();
                if matches!(
                    uppercase.as_str(),
                    "AS" | "INNER"
                        | "JOIN"
                        | "ON"
                        | "ORDER"
                        | "BY"
                        | "ASC"
                        | "DESC"
                        | "GROUP"
                        | "COUNT"
                        | "SUM"
                        | "MIN"
                        | "MAX"
                        | "CREATE"
                        | "TABLE"
                        | "INTEGER"
                        | "TEXT"
                        | "PRIMARY"
                        | "KEY"
                        | "NOT"
                        | "NULL"
                        | "INSERT"
                        | "INTO"
                        | "VALUES"
                        | "SELECT"
                        | "FROM"
                        | "WHERE"
                        | "UPDATE"
                        | "SET"
                        | "DELETE"
                        | "AND"
                        | "OR"
                        | "IS"
                ) {
                    TokenKind::Keyword(uppercase)
                } else {
                    TokenKind::Identifier(word.to_ascii_lowercase())
                }
            }
            _ => {
                return Err(SqlError::new(
                    source,
                    start..start + character.len_utf8(),
                    format!("unexpected character {character:?}"),
                ));
            }
        };
        let end = characters
            .peek()
            .map_or(source.len(), |&(offset, _)| offset);
        tokens.push(Token {
            kind,
            span: start..end,
        });
    }
    Ok(tokens)
}
