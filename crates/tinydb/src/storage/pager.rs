use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::header::{Header, invalid_data};
use super::{PAGE_SIZE, Page, PageId};

/// Owns one database file and transfers whole pages between memory and disk.
///
/// Use a single pager per file; concurrent access is not coordinated. Call
/// [`Self::sync`] to persist changes and surface synchronization errors. Writes
/// are not transactional. Discard the pager after a write or sync error.
pub struct Pager {
    file: File,
    header: Header,
}

impl Pager {
    /// Creates a new database containing only a metadata page.
    ///
    /// Returns `AlreadyExists` without overwriting an existing file.
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        let header = Header { page_count: 0 };
        file.write_all(&header.encode())?;
        Ok(Self { file, header })
    }

    /// Opens an existing database and validates its header and exact file size.
    ///
    /// Invalid or truncated databases return `InvalidData`; missing files are
    /// not created. The file must be writable.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < PAGE_SIZE as u64 {
            return Err(invalid_data("truncated database metadata page"));
        }
        let mut bytes = [0; PAGE_SIZE];
        file.read_exact(&mut bytes)?;
        let header = Header::decode(&bytes)?;
        if file_len != header.file_len()? {
            return Err(invalid_data("file length does not match header page count"));
        }
        Ok(Self { file, header })
    }

    /// Returns the number of allocated data pages, excluding metadata page 0.
    pub fn page_count(&self) -> u64 {
        self.header.page_count
    }

    /// Appends a zero-filled page and returns its stable, nonzero page ID.
    ///
    /// The page is written before its header count. This operation is not
    /// crash-atomic; a partial failure can leave an invalid file.
    pub fn allocate_page(&mut self) -> io::Result<PageId> {
        let page_id = self.header.page_count.checked_add(1).ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidInput, "database page count exhausted")
        })?;
        let header = Header {
            page_count: page_id,
        };
        // Check the new file length before performing either write.
        header.file_len()?;
        self.file.seek(SeekFrom::Start(self.header.file_len()?))?;
        self.file.write_all(&[0; PAGE_SIZE])?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header.encode())?;
        self.header = header;
        Ok(page_id)
    }

    /// Reads exactly one allocated data page. Page 0 is not accessible.
    pub fn read_page(&mut self, page_id: PageId) -> io::Result<Page> {
        let offset = self.page_offset(page_id)?;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut page = [0; PAGE_SIZE];
        self.file.read_exact(&mut page)?;
        Ok(page)
    }

    /// Replaces exactly one allocated data page without changing the header.
    pub fn write_page(&mut self, page_id: PageId, page: &Page) -> io::Result<()> {
        let offset = self.page_offset(page_id)?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(page)
    }

    /// Synchronizes file contents and metadata with the underlying storage.
    pub fn sync(&self) -> io::Result<()> {
        self.file.sync_all()
    }

    fn page_offset(&self, page_id: PageId) -> io::Result<u64> {
        if page_id == 0 || page_id > self.header.page_count {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("page {page_id} is outside 1..={}", self.header.page_count),
            ));
        }
        // open/allocate validate the file length, which also bounds each offset.
        Ok(page_id * PAGE_SIZE as u64)
    }
}
