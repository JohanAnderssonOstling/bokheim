//! Extracted text is transient; only unit/token checkpoints enter the index.
use crate::PdfAnnotationRect;
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BookTextMatch {
    pub excerpt: String,
    pub excerpt_matches: Vec<Range<usize>>,
    pub target: Option<String>,
}

/// Ordered source parts covering a phrase, starting at a local token offset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTextLocation {
    pub documents: Vec<String>,
    pub token_offset: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTextPart {
    pub document: String,
    pub text: String,
    /// Optional PDF character bounds, populated only for a requested result page.
    pub bounds: Vec<Option<PdfAnnotationRect>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTextTarget {
    pub document: String,
    pub character_offset: usize,
}
impl BookTextTarget {
    pub fn open_target(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self).map(|value| format!("book-text:{value}"))
    }
    pub fn from_open_target(value: &str) -> Option<Self> {
        serde_json::from_str(value.strip_prefix("book-text:")?).ok()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTextCheckpoint {
    pub document: String,
    pub start: usize,
}

/// CPU-prepared input for one atomic FTS publication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTextIndex {
    pub text: String,
    pub checkpoints: Vec<BookTextCheckpoint>,
}
