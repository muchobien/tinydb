//! Persistent storage primitives. See `docs/storage-format.md` for the file format.

mod header;
mod page;
mod pager;

pub use page::{PAGE_SIZE, Page, PageId};
pub use pager::Pager;
