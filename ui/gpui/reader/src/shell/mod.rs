//! Mechanics every reader window needs, written once.
//!
//! EPUB, PDF and CBZ windows differ in how they present a book, not in how
//! they remember where the window was or how they write to the library. Those
//! parts live here so a fix lands in one reader instead of three.

pub(crate) mod chrome;
pub(crate) mod geometry;
pub(crate) mod persistence;
