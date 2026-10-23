//! Persistent storage primitives. See `docs/storage-format.md` for the file format.

mod header;
mod page;
mod pager;
mod record;
mod record_store;
mod slotted_page;

pub use page::{PAGE_SIZE, Page, PageId};
pub use pager::Pager;
pub use record::{Record, Value};
pub use record_store::{RecordId, RecordStore};
pub use slotted_page::{MAX_RECORD_SIZE, SlotId, SlottedPage};
