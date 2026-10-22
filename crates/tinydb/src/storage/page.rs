/// Size in bytes of every on-disk page, including the metadata page.
pub const PAGE_SIZE: usize = 4096;

/// A full page of uninterpreted bytes.
pub type Page = [u8; PAGE_SIZE];

/// A physical page number. Page 0 is reserved for database metadata.
pub type PageId = u64;
