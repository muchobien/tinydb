use std::io::{self, ErrorKind};

use super::header::invalid_data;
use super::{PAGE_SIZE, Page};

const HEADER_SIZE: usize = 16;
const SLOT_SIZE: usize = 4;
const MAGIC: &[u8; 4] = b"SLT1";

/// Largest encoded record that fits on an otherwise empty slotted page.
pub const MAX_RECORD_SIZE: usize = PAGE_SIZE - HEADER_SIZE - SLOT_SIZE;

/// A stable index into a page's slot directory. Slots are never reused.
pub type SlotId = u16;

/// A validated slotted page whose payloads remain packed after every mutation.
pub struct SlottedPage {
    bytes: Page,
}

impl Default for SlottedPage {
    fn default() -> Self {
        Self::new()
    }
}

impl SlottedPage {
    pub fn new() -> Self {
        let mut bytes = [0; PAGE_SIZE];
        bytes[..4].copy_from_slice(MAGIC);
        put_u16(&mut bytes, 6, PAGE_SIZE as u16);
        Self { bytes }
    }

    /// Validates all directory entries and payload ranges before accepting bytes.
    pub fn from_page(bytes: Page) -> io::Result<Self> {
        if &bytes[..4] != MAGIC || bytes[8..HEADER_SIZE].iter().any(|&b| b != 0) {
            return Err(invalid_data("invalid slotted-page header"));
        }
        let page = Self { bytes };
        let directory_end = HEADER_SIZE + usize::from(page.slot_count()) * SLOT_SIZE;
        let data_start = page.data_start();
        if directory_end > data_start || data_start > PAGE_SIZE {
            return Err(invalid_data("slotted-page directory exceeds free space"));
        }
        if page.bytes[directory_end..data_start]
            .iter()
            .any(|&b| b != 0)
        {
            return Err(invalid_data("nonzero slotted-page free bytes"));
        }
        let mut ranges = Vec::new();
        for slot in 0..page.slot_count() {
            let (offset, len) = page.entry(slot);
            if offset == 0 && len == 0 {
                continue;
            }
            if len == 0 || offset < data_start || offset + len > PAGE_SIZE {
                return Err(invalid_data("invalid slotted-page record range"));
            }
            ranges.push((offset, offset + len));
        }
        ranges.sort_unstable();
        let mut end = data_start;
        for (start, next_end) in ranges {
            if start != end {
                return Err(invalid_data("overlap or gap in slotted-page payloads"));
            }
            end = next_end;
        }
        if end != PAGE_SIZE {
            return Err(invalid_data("incomplete slotted-page payloads"));
        }
        Ok(page)
    }

    pub fn as_page(&self) -> &Page {
        &self.bytes
    }

    /// Counts live entries and tombstones, since slot IDs are never reused.
    pub fn slot_count(&self) -> u16 {
        get_u16(&self.bytes, 4)
    }

    /// Bytes available for payload growth or new payloads plus directory entries.
    pub fn free_space(&self) -> usize {
        self.data_start() - HEADER_SIZE - usize::from(self.slot_count()) * SLOT_SIZE
    }

    pub fn read(&self, slot: SlotId) -> io::Result<&[u8]> {
        if slot >= self.slot_count() {
            return Err(missing_slot());
        }
        let (offset, len) = self.entry(slot);
        if len == 0 {
            return Err(missing_slot());
        }
        Ok(&self.bytes[offset..offset + len])
    }

    /// Inserts bytes under a new slot ID. Capacity errors leave the page intact.
    pub fn insert(&mut self, record: &[u8]) -> io::Result<SlotId> {
        validate_payload(record)?;
        if record.len() + SLOT_SIZE > self.free_space() {
            return Err(full_page());
        }
        let slot = self.slot_count();
        self.rebuild(slot, Some(record), true);
        Ok(slot)
    }

    /// Updates in the same slot; returns `StorageFull` if growth does not fit.
    pub fn update(&mut self, slot: SlotId, record: &[u8]) -> io::Result<()> {
        let old_len = self.read(slot)?.len();
        validate_payload(record)?;
        if record.len() > self.free_space() + old_len {
            return Err(full_page());
        }
        self.rebuild(slot, Some(record), false);
        Ok(())
    }

    /// Retires a slot permanently and immediately reclaims its payload bytes.
    pub fn delete(&mut self, slot: SlotId) -> io::Result<()> {
        self.read(slot)?;
        self.rebuild(slot, None, false);
        Ok(())
    }

    fn data_start(&self) -> usize {
        usize::from(get_u16(&self.bytes, 6))
    }

    fn entry(&self, slot: SlotId) -> (usize, usize) {
        let start = HEADER_SIZE + usize::from(slot) * SLOT_SIZE;
        (
            usize::from(get_u16(&self.bytes, start)),
            usize::from(get_u16(&self.bytes, start + 2)),
        )
    }

    // All validation/capacity checks happen before this infallible rebuild.
    fn rebuild(&mut self, changed: SlotId, replacement: Option<&[u8]>, append: bool) {
        let count = self.slot_count() + u16::from(append);
        let mut next = Self::new();
        put_u16(&mut next.bytes, 4, count);
        let mut data_start = PAGE_SIZE;
        for slot in 0..count {
            let record = if slot == changed {
                replacement
            } else {
                self.read(slot).ok()
            };
            if let Some(record) = record {
                data_start -= record.len();
                next.bytes[data_start..data_start + record.len()].copy_from_slice(record);
                let entry = HEADER_SIZE + usize::from(slot) * SLOT_SIZE;
                put_u16(&mut next.bytes, entry, data_start as u16);
                put_u16(&mut next.bytes, entry + 2, record.len() as u16);
            }
        }
        put_u16(&mut next.bytes, 6, data_start as u16);
        *self = next;
    }
}

fn get_u16(bytes: &Page, offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn put_u16(bytes: &mut Page, offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn validate_payload(record: &[u8]) -> io::Result<()> {
    if record.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "empty slot payload",
        ));
    }
    if record.len() > MAX_RECORD_SIZE {
        return Err(full_page());
    }
    Ok(())
}

fn full_page() -> io::Error {
    io::Error::new(
        ErrorKind::StorageFull,
        "record does not fit in slotted page",
    )
}

fn missing_slot() -> io::Error {
    io::Error::new(ErrorKind::NotFound, "record slot is missing or deleted")
}
