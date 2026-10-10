//! What an audiobook player needs to open a book without inspecting its file.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybackMetadata {
    #[serde(default = "default_audio_format")]
    pub format: book_model::BookFormat,
    pub title: String,
    pub author: String,
    pub narrator: Option<String>,
    pub duration_ms: u64,
    pub chapters: Vec<book_model::AudiobookChapter>,
    #[serde(default)]
    pub tracks: Vec<book_model::AudiobookTrack>,
    #[serde(with = "serde_bytes")]
    pub cover: Vec<u8>,
}

fn default_audio_format() -> book_model::BookFormat {
    book_model::BookFormat::M4b
}
