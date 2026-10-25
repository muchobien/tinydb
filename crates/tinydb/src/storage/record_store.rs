use std::io::{self, ErrorKind};
use std::path::Path;

use super::{MAX_RECORD_SIZE, PageId, Pager, Record, SlotId, SlottedPage};

/// A stable physical address. Deleting a record never makes this ID reusable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecordId {
    pub page_id: PageId,
    pub slot_id: SlotId,
}

/// A single-owner heap of records over slotted pages.
///
/// There are no transactions or concurrent writers. Call [`Self::sync`] to
/// persist changes and discard the store after write/sync errors. Capacity or
/// validation errors do not modify records.
pub struct RecordStore {
    pager: Pager,
}

impl RecordStore {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            pager: Pager::create(path)?,
        })
    }

    /// Validates every slotted page and live record before returning the store.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut pager = Pager::open(path)?;
        for id in 1..=pager.page_count() {
            let page = SlottedPage::from_page(pager.read_page(id)?)?;
            for slot in 0..page.slot_count() {
                match page.read(slot) {
                    Ok(bytes) => {
                        Record::decode(bytes)?;
                    }
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(Self { pager })
    }

    /// Scans existing pages for space, appending a page only when necessary.
    pub fn insert(&mut self, record: &Record) -> io::Result<RecordId> {
        let bytes = encode_for_page(record)?;
        for page_id in 1..=self.pager.page_count() {
            let mut page = SlottedPage::from_page(self.pager.read_page(page_id)?)?;
            match page.insert(&bytes) {
                Ok(slot_id) => {
                    self.pager.write_page(page_id, page.as_page())?;
                    return Ok(RecordId { page_id, slot_id });
                }
                Err(error) if error.kind() == ErrorKind::StorageFull => continue,
                Err(error) => return Err(error),
            }
        }
        let mut page = SlottedPage::new();
        let slot_id = page.insert(&bytes)?;
        let page_id = self.pager.allocate_page()?;
        self.pager.write_page(page_id, page.as_page())?;
        Ok(RecordId { page_id, slot_id })
    }

    pub fn read(&mut self, id: RecordId) -> io::Result<Record> {
        let page = SlottedPage::from_page(self.pager.read_page(id.page_id)?)?;
        Record::decode(page.read(id.slot_id)?)
    }

    pub fn scan(&mut self) -> io::Result<Vec<(RecordId, Record)>> {
        let mut records = Vec::new();
        for page_id in 1..=self.pager.page_count() {
            let page = SlottedPage::from_page(self.pager.read_page(page_id)?)?;
            for slot_id in 0..page.slot_count() {
                match page.read(slot_id) {
                    Ok(bytes) => {
                        records.push((RecordId { page_id, slot_id }, Record::decode(bytes)?))
                    }
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(records)
    }

    /// Replaces a record without changing its ID or moving it to another page.
    pub fn update(&mut self, id: RecordId, record: &Record) -> io::Result<()> {
        let mut page = SlottedPage::from_page(self.pager.read_page(id.page_id)?)?;
        page.read(id.slot_id)?;
        let bytes = encode_for_page(record)?;
        page.update(id.slot_id, &bytes)?;
        self.pager.write_page(id.page_id, page.as_page())
    }

    pub fn delete(&mut self, id: RecordId) -> io::Result<()> {
        let mut page = SlottedPage::from_page(self.pager.read_page(id.page_id)?)?;
        page.delete(id.slot_id)?;
        self.pager.write_page(id.page_id, page.as_page())
    }

    pub fn page_count(&self) -> u64 {
        self.pager.page_count()
    }

    pub fn sync(&self) -> io::Result<()> {
        self.pager.sync()
    }
}

fn encode_for_page(record: &Record) -> io::Result<Vec<u8>> {
    let bytes = record.encode()?;
    if bytes.len() > MAX_RECORD_SIZE {
        return Err(io::Error::new(
            ErrorKind::StorageFull,
            "encoded record exceeds single-page capacity",
        ));
    }
    Ok(bytes)
}
