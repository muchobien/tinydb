use std::io::{self, ErrorKind};

use super::header::invalid_data;

/// Values supported by the initial, schema-independent record format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Integer(i64),
    Text(String),
}

/// An ordered collection of self-describing values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub values: Vec<Value>,
}

impl Record {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// Encodes explicit little-endian fields, never Rust struct memory.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let count = u16::try_from(self.values.len())
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "too many record values"))?;
        let mut bytes = count.to_le_bytes().to_vec();
        for value in &self.values {
            match value {
                Value::Null => bytes.push(0),
                Value::Integer(value) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                Value::Text(value) => {
                    let len = u32::try_from(value.len()).map_err(|_| {
                        io::Error::new(ErrorKind::InvalidInput, "record text is too long")
                    })?;
                    bytes.push(2);
                    bytes.extend_from_slice(&len.to_le_bytes());
                    bytes.extend_from_slice(value.as_bytes());
                }
            }
        }
        Ok(bytes)
    }

    /// Decodes one complete record, rejecting truncated or trailing bytes.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut remaining = bytes;
        let count = u16::from_le_bytes(take(&mut remaining, 2)?.try_into().unwrap());
        // Every value needs at least a tag. Check before allocating the vector.
        if usize::from(count) > remaining.len() {
            return Err(invalid_data("truncated record values"));
        }
        let mut values = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let value = match take(&mut remaining, 1)?[0] {
                0 => Value::Null,
                1 => Value::Integer(i64::from_le_bytes(
                    take(&mut remaining, 8)?.try_into().unwrap(),
                )),
                2 => {
                    let len = u32::from_le_bytes(take(&mut remaining, 4)?.try_into().unwrap());
                    let len = usize::try_from(len)
                        .map_err(|_| invalid_data("text length exceeds address range"))?;
                    let text = std::str::from_utf8(take(&mut remaining, len)?)
                        .map_err(|_| invalid_data("invalid record UTF-8"))?;
                    Value::Text(text.to_owned())
                }
                _ => return Err(invalid_data("unknown record value tag")),
            };
            values.push(value);
        }
        if !remaining.is_empty() {
            return Err(invalid_data("trailing bytes after record"));
        }
        Ok(Self { values })
    }
}

fn take<'a>(remaining: &mut &'a [u8], len: usize) -> io::Result<&'a [u8]> {
    if len > remaining.len() {
        return Err(invalid_data("truncated record field"));
    }
    let (field, rest) = remaining.split_at(len);
    *remaining = rest;
    Ok(field)
}
