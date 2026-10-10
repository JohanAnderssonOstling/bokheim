use std::time::Duration;

/// A chapter is an interval of the book-wide playback clock, in milliseconds.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AudiobookChapter {
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl AudiobookChapter {
    pub fn start(&self) -> Duration {
        Duration::from_millis(self.start_ms)
    }

    pub fn end(&self) -> Duration {
        Duration::from_millis(self.end_ms)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChapterOrigin {
    EmbeddedM4b,
    TrackFiles,
    CueSheet,
    Audnexus,
    FullAudiobookFallback,
}
