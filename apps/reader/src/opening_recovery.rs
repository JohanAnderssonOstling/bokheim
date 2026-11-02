//! Bounded, non-destructive recovery while opening a book.

use ui_text::text;

use app::BackendError;
use html::resources::ResourceProvider;
use skrift_column_paginator::{RendererInitialConfig, RendererPreparation};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct OpeningRecovery {
    saved_position_unavailable: bool,
    alternate_chapter: Option<String>,
}

impl OpeningRecovery {
    pub(crate) fn restore_position<T>(&mut self, result: Result<Option<T>, BackendError>, explicit_target: bool) -> Option<T> {
        match result {
            Ok(position) => position,
            Err(error) => {
                log::warn!("could not restore reading position; opening without it: {error}");
                // Selecting a contents entry is an intentional new position.
                self.saved_position_unavailable = !explicit_target;
                None
            }
        }
    }

    pub(crate) fn save_progress(&self) -> bool {
        !self.saved_position_unavailable && self.alternate_chapter.is_none()
    }

    pub(crate) fn notice(&self) -> Option<String> {
        let reason = match &self.alternate_chapter {
            Some(chapter) => format!(text!(reader::opened_because_the_requested_chapter_couldn_t_be_displayed), chapter = chapter),
            None if self.saved_position_unavailable => text!(reader::opened_at_the_beginning_because_your_saved_reading_position_couldn_t_be).to_owned(),
            None => return None,
        };
        Some(format!(text!(reader::your_previous_progress_has_been_kept_reading_progress_won_t_be_saved), reason = reason))
    }

    pub(crate) fn prepare_document(&mut self, provider: Arc<dyn ResourceProvider>, documents: &[String], nav_state: Option<&str>, initial_target: Option<&str>, mut config: RendererInitialConfig) -> Result<RendererPreparation, String> {
        let defaults = RendererInitialConfig::default();
        if !config.font_size.is_finite() {
            config.font_size = defaults.font_size;
        }
        if !config.column_width.is_finite() {
            config.column_width = defaults.column_width;
        }
        if !config.scale.is_finite() {
            config.scale = defaults.scale;
        }
        let original = match RendererPreparation::from_provider_with_nav_and_target(provider.clone(), documents.to_vec(), 0, nav_state, initial_target, config.clone()) {
            Ok(prepared) => return Ok(prepared),
            Err(error) => error,
        };
        // Text resources are already prepared. Limit fallback work even if a
        // provider is unavailable or a large publication has many bad pages.
        for (index, uri) in documents.iter().take(3).enumerate() {
            if let Ok(prepared) = RendererPreparation::from_provider_with_nav_and_target(provider.clone(), documents.to_vec(), index, None, None, config.clone()) {
                log::warn!("requested chapter failed; recovered at {uri}: {original}");
                self.alternate_chapter = Some(uri.clone());
                return Ok(prepared);
            }
        }
        Err(original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use html::resources::ResourceMetadata;
    use std::{io, sync::Mutex};

    struct Chapters {
        reads: Mutex<Vec<String>>,
        unavailable: Vec<String>,
    }

    impl ResourceProvider for Chapters {
        fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
            self.reads.lock().unwrap().push(uri.to_owned());
            if self.unavailable.iter().any(|missing| missing == uri) { Err(io::Error::new(io::ErrorKind::NotFound, "chapter missing")) } else { Ok(b"<html><body><p>Readable chapter</p></body></html>".to_vec()) }
        }
        fn metadata(&self, _: &str) -> io::Result<ResourceMetadata> {
            Ok(ResourceMetadata { media_type: Some("text/html".into()), charset: None })
        }
        fn exists(&self, uri: &str) -> bool {
            !self.unavailable.iter().any(|missing| missing == uri)
        }
        fn resolve(&self, _: &str, href: &str) -> String {
            href.to_owned()
        }
        fn list_html_candidates(&self, _: &str) -> io::Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    fn chapters(unavailable: &[&str]) -> Arc<Chapters> {
        Arc::new(Chapters { reads: Mutex::default(), unavailable: unavailable.iter().map(|value| (*value).to_owned()).collect() })
    }

    #[test]
    fn missing_saved_state_protects_progress_unless_navigation_is_explicit() {
        for explicit in [false, true] {
            let mut recovery = OpeningRecovery::default();
            assert_eq!(recovery.restore_position::<String>(Err(BackendError::message("database unavailable")), explicit), None);
            assert_eq!(recovery.save_progress(), explicit);
            assert_eq!(recovery.notice().is_none(), explicit);
        }
        let mut recovery = OpeningRecovery::default();
        assert_eq!(recovery.restore_position(Ok(Some("2:40")), false), Some("2:40"));
        assert!(recovery.save_progress());
    }

    #[test]
    fn good_saved_chapters_are_used_without_probing_other_documents() {
        let provider = chapters(&[]);
        let documents = vec!["cover.html".into(), "chapter.html".into()];
        let mut recovery = OpeningRecovery::default();
        recovery.prepare_document(provider.clone(), &documents, Some("1:20"), None, RendererInitialConfig::default()).unwrap();
        assert_eq!(*provider.reads.lock().unwrap(), ["chapter.html"]);
        assert!(recovery.save_progress());
    }

    #[test]
    fn broken_requested_chapters_recover_with_a_notice_and_protected_progress() {
        let provider = chapters(&["cover.html"]);
        let documents = vec!["cover.html".into(), "chapter.html".into()];
        let mut recovery = OpeningRecovery::default();
        recovery.prepare_document(provider.clone(), &documents, None, Some("cover.html#title"), RendererInitialConfig::default()).unwrap();
        assert_eq!(provider.reads.lock().unwrap().last().map(String::as_str), Some("chapter.html"));
        assert!(!recovery.save_progress());
        assert!(recovery.notice().unwrap().contains("chapter.html"));
    }

    #[test]
    fn unusable_publications_have_bounded_fallbacks_and_keep_the_original_error() {
        let documents: Vec<String> = (0..100).map(|index| format!("{index}.html")).collect();
        let provider = Arc::new(Chapters { reads: Mutex::default(), unavailable: documents.clone() });
        let mut recovery = OpeningRecovery::default();
        let error = recovery.prepare_document(provider.clone(), &documents, Some("90:0"), None, RendererInitialConfig::default()).err().unwrap();
        assert!(error.contains("90.html"));
        assert_eq!(provider.reads.lock().unwrap().len(), 4);
        assert!(recovery.notice().is_none());
    }

    #[test]
    fn invalid_layout_dimensions_use_defaults_without_moving_the_saved_position() {
        let provider = chapters(&[]);
        let documents = vec!["cover.html".into(), "chapter.html".into()];
        let config = RendererInitialConfig { font_size: f32::NAN, column_width: f64::NAN, scale: f64::NAN, ..Default::default() };
        let mut recovery = OpeningRecovery::default();
        recovery.prepare_document(provider.clone(), &documents, Some("1:20"), None, config).unwrap();
        assert_eq!(*provider.reads.lock().unwrap(), ["chapter.html"]);
        assert!(recovery.save_progress());
    }
}
