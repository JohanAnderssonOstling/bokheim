//! Mechanics every reader surface needs, written once.
//!
//! EPUB and PDF differ in how they present a book, but share persistence,
//! annotation, marks, and chrome mechanics. Those parts live here so a fix
//! lands in one reader instead of three.

pub(crate) mod annotation_editor;
pub(crate) mod chrome;
pub(crate) mod marks;
pub(crate) mod persistence;
pub(crate) mod state_panel;
