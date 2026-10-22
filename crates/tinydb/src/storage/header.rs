use std::io::{self, ErrorKind};

use super::{PAGE_SIZE, Page};

const MAGIC: &[u8; 8] = b"TINYDB\0\0";
const FORMAT_VERSION: u32 = 1;

/// The only changing field in the version 1 header.
pub(super) struct Header {
    pub page_count: u64,
}

impl Header {
    pub fn encode(&self) -> Page {
        let mut bytes = [0; PAGE_SIZE];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes[12..16].copy_from_slice(&(PAGE_SIZE as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&self.page_count.to_le_bytes());
        bytes
    }

    pub fn decode(bytes: &Page) -> io::Result<Self> {
        if &bytes[..8] != MAGIC {
            return Err(invalid_data("invalid TinyDB magic bytes"));
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if version != FORMAT_VERSION {
            return Err(invalid_data("unsupported TinyDB format version"));
        }
        let page_size = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        if page_size != PAGE_SIZE as u32 {
            return Err(invalid_data("unsupported TinyDB page size"));
        }
        if bytes[24..].iter().any(|&byte| byte != 0) {
            return Err(invalid_data("nonzero reserved header bytes"));
        }
        Ok(Self {
            page_count: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        })
    }

    pub fn file_len(&self) -> io::Result<u64> {
        self.page_count
            .checked_add(1)
            .and_then(|count| count.checked_mul(PAGE_SIZE as u64))
            .ok_or_else(|| invalid_data("page count exceeds the file offset range"))
    }
}

pub(super) fn invalid_data(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}
