#[path = "../crates/html/src/book.rs"]
mod adapter;
pub(crate) use adapter::BookSource as NativeBook;
