//! Persistent segment data for text editing, caret navigation, and parsed
//! annotations.
//!
//! Parsing and analysis algorithms live in `segment-parser`; this crate owns
//! the data model and its invariant-preserving API:
//! - lossless runs
//! - `TokenizedSegment` for lexical atomic units
//! - `SegmentTree` for hierarchical parser output

mod tree;

pub use tree::*;
#[path = "segment_pipeline.rs"]
mod segment_pipeline;
pub use segment_pipeline::{structured_tag, InlinePlaceable, StructuredElement, StructuredSegment, TokenizedSegment};
