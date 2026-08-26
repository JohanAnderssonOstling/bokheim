//! Device and synchronized application preferences.

pub use app_contract::preferences::*;
use std::fs;
use std::io;
use std::path::PathBuf;

const PREFERENCES_FILE: &str = "reader_preferences.json";
const BROWSING_PREFERENCES_FILE: &str = "browsing_preferences.json";
impl crate::app::BackendContext {
    pub fn browsing_preferences_path(&self) -> PathBuf {
        self.app_data_dir().join(BROWSING_PREFERENCES_FILE)
    }

    pub fn load_browsing_preferences(&self) -> io::Result<BrowsingPreferences> {
        match fs::read(self.browsing_preferences_path()) {
            Ok(bytes) => serde_json::from_slice::<BrowsingPreferences>(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BrowsingPreferences::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save_browsing_preferences(&self, preferences: &BrowsingPreferences) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        super::persistence::write_json_atomic(&self.browsing_preferences_path(), &bytes)
    }

    pub fn apply_browsing_preference_patch(&self, patch: BrowsingPreferencePatch) -> io::Result<BrowsingPreferences> {
        let mut preferences = self.load_browsing_preferences()?;
        patch.apply(&mut preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.save_browsing_preferences(&preferences)?;
        Ok(preferences)
    }

    pub fn reader_preferences_path(&self) -> PathBuf {
        self.app_data_dir().join(PREFERENCES_FILE)
    }

    pub fn load_reader_preferences(&self) -> io::Result<ReaderPreferences> {
        match fs::read(self.reader_preferences_path()) {
            Ok(bytes) => serde_json::from_slice::<ReaderPreferences>(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ReaderPreferences::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save_reader_preferences(&self, preferences: ReaderPreferences) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        super::persistence::write_json_atomic(&self.reader_preferences_path(), &bytes)
    }

    pub fn apply_reader_preference_patch(&self, patch: ReaderPreferencePatch) -> io::Result<ReaderPreferences> {
        let mut preferences = self.load_reader_preferences()?;
        patch.apply(&mut preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.save_reader_preferences(preferences)?;
        Ok(preferences)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsing_theme_is_a_closed_persisted_preference() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::Theme(ApplicationTheme::LibrarySerif)).unwrap();
        assert_eq!(updated.theme(), ApplicationTheme::LibrarySerif);
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        assert!(serde_json::from_str::<BrowsingPreferences>(r#"{"theme":"unknown"}"#).is_err());
    }

    #[test]
    fn catalogue_language_is_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        assert_eq!(context.load_browsing_preferences().unwrap().included_catalog_languages(), ["en"]);
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::IncludedCatalogLanguages(vec!["sv".to_owned()])).unwrap();
        assert_eq!(updated.included_catalog_languages(), ["sv"]);
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
    }

    #[test]
    fn interface_font_is_the_single_closed_persisted_ui_font() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::InterfaceFont(UiFontFamily::Serif)).unwrap();
        assert_eq!(updated.interface_font(), UiFontFamily::Serif);
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        assert!(serde_json::from_str::<BrowsingPreferences>(r#"{"interface_font":"unknown"}"#).is_err());
    }

    #[test]
    fn browsing_options_cover_every_closed_value_once() {
        let options = BrowsingPreferenceOptions::default();
        assert_eq!(options.themes.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ApplicationTheme::GrecoFuturistic, ApplicationTheme::LibrarySerif, ApplicationTheme::ClassicWhite]);
        assert!(options.themes.iter().all(|option| !option.label.is_empty() && !option.description.is_empty()));
        assert_eq!(ApplicationTheme::GrecoFuturistic.next(), ApplicationTheme::LibrarySerif);
        assert_eq!(ApplicationTheme::LibrarySerif.next(), ApplicationTheme::ClassicWhite);
        assert_eq!(ApplicationTheme::ClassicWhite.next(), ApplicationTheme::GrecoFuturistic);
        assert_eq!(options.fonts.iter().map(|option| option.value).collect::<Vec<_>>(), vec![UiFontFamily::SansSerif, UiFontFamily::Serif]);
        assert_eq!(options.fonts.iter().map(|option| option.label.as_str()).collect::<Vec<_>>(), vec!["Sans Serif", "Serif"]);
        assert!(options.fonts.iter().all(|option| !option.font_name.is_empty()));
        assert!(options.fonts.iter().all(|option| !option.generic_family.is_empty()));
    }

    #[test]
    fn old_browsing_preferences_receive_the_interface_font_default() {
        let preferences: BrowsingPreferences = serde_json::from_str(r#"{"theme":"library_serif"}"#).unwrap();
        assert_eq!(preferences.theme(), ApplicationTheme::LibrarySerif);
        assert_eq!(preferences.interface_font(), UiFontFamily::SansSerif);
        assert_eq!(preferences.ui_font_size_px(), UI_FONT_SIZE_DEFAULT);
        assert_eq!(preferences.included_catalog_languages(), ["en"]);
    }

    #[test]
    fn ui_font_size_is_bounded_and_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MIN)).unwrap();
        assert_eq!(updated.ui_font_size_px(), UI_FONT_SIZE_MIN);
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        assert!(context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MIN - 1)).is_err());
        assert!(context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MAX + 1)).is_err());
        assert_eq!(context.load_browsing_preferences().unwrap(), updated);
        let updated = context.apply_browsing_preference_patch(BrowsingPreferencePatch::UiFontSizePx(UI_FONT_SIZE_MAX)).unwrap();
        assert_eq!(updated.ui_font_size_px(), UI_FONT_SIZE_MAX);
        assert!(serde_json::from_str::<BrowsingPreferences>(&format!(r#"{{"ui_font_size_px":{}}}"#, UI_FONT_SIZE_MIN - 1)).is_err());
    }

    #[test]
    fn exact_interface_font_preferences_migrate_and_legacy_heading_fonts_are_ignored() {
        let preferences: BrowsingPreferences = serde_json::from_str(r#"{"interface_font":"ibm_plex_sans","heading_font":"source_serif_4"}"#).unwrap();
        assert_eq!(preferences.interface_font(), UiFontFamily::SansSerif);
        let preferences: BrowsingPreferences = serde_json::from_str(r#"{"interface_font":"liberation_serif","heading_font":"liberation_sans"}"#).unwrap();
        assert_eq!(preferences.interface_font(), UiFontFamily::Serif);
    }

    #[test]
    fn rejects_invalid_numeric_preferences() {
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"font_size_px":null}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"max_line_width_px":9000.0}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"zoom":0.1}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"line_height":9.0}"#).is_err());
        assert!(serde_json::from_str::<ReaderPreferences>(r#"{"max_column_count":0}"#).is_err());
    }

    #[test]
    fn missing_json_fields_receive_current_defaults() {
        let preferences: ReaderPreferences = serde_json::from_str(r#"{"font_size_px":24.0}"#).unwrap();
        assert_eq!(preferences.font_size_px(), 24.0);
        assert_eq!(preferences.max_line_width_px(), 600.0);
        assert_eq!(preferences.pdf_zoom_mode(), PdfZoomMode::FitWidth);
        assert_eq!(preferences.pdf_page_count(), 1);
        assert_eq!(preferences.pdf_zoom(), 1.0);
        assert!(preferences.pdf_trim_margins());
    }

    #[test]
    fn atomic_preferences_write_replaces_the_complete_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reader_preferences.json");
        super::super::persistence::write_json_atomic(&path, br#"{"font_size_px":20.0}"#).unwrap();
        super::super::persistence::write_json_atomic(&path, br#"{"font_size_px":24.0}"#).unwrap();
        assert_eq!(fs::read(&path).unwrap(), br#"{"font_size_px":24.0}"#);
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn client_preference_contexts_do_not_leak_state_between_instances() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(first_dir.path().to_path_buf())).unwrap();
        let second = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(second_dir.path().to_path_buf())).unwrap();
        let mut first_preferences = ReaderPreferences::default();
        first_preferences.set_zoom(1.75).unwrap();
        first.save_reader_preferences(first_preferences).unwrap();

        assert_eq!(first.load_reader_preferences().unwrap().zoom(), 1.75);
        assert_eq!(second.load_reader_preferences().unwrap().zoom(), ReaderPreferences::default().zoom());
        assert!(!second.reader_preferences_path().exists());
    }

    #[test]
    fn malformed_persisted_preferences_fail_closed_without_being_rewritten() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let malformed = br#"{"zoom":99.0}"#;
        fs::write(context.reader_preferences_path(), malformed).unwrap();

        let error = context.load_reader_preferences().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(context.reader_preferences_path()).unwrap(), malformed);
    }

    #[test]
    fn preference_patch_validates_before_atomically_replacing_persisted_state() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        let updated = context.apply_reader_preference_patch(ReaderPreferencePatch::FontSizePx(28.0)).unwrap();
        assert_eq!(updated.font_size_px(), 28.0);

        let before_invalid_patch = fs::read(context.reader_preferences_path()).unwrap();
        let error = context.apply_reader_preference_patch(ReaderPreferencePatch::FontSizePx(100.0)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read(context.reader_preferences_path()).unwrap(), before_invalid_patch);
    }

    #[test]
    fn reader_capabilities_make_renderer_specific_support_explicit() {
        let native = ReaderCapabilities::for_engine(ReaderEngine::Native);
        let foliate = ReaderCapabilities::for_engine(ReaderEngine::Foliate);
        assert!(native.supports(ReaderPreferenceKey::PdfZoomMode));
        assert!(native.supports(ReaderPreferenceKey::PdfPageCount));
        assert!(native.supports(ReaderPreferenceKey::PdfZoom));
        assert!(native.supports(ReaderPreferenceKey::PdfTrimMargins));
        assert!(native.supports(ReaderPreferenceKey::LineHeight));
        assert!(native.supports(ReaderPreferenceKey::FontFamily));
        assert!(native.supports(ReaderPreferenceKey::TextAlignment));
        assert!(!native.supports(ReaderPreferenceKey::Hyphenation));
        assert!(native.supports(ReaderPreferenceKey::MaxColumnCount));
        assert!(!foliate.supports(ReaderPreferenceKey::PdfZoomMode));
        assert!(!foliate.supports(ReaderPreferenceKey::PdfTrimMargins));
        assert!(foliate.supports(ReaderPreferenceKey::Hyphenation));
    }

    #[test]
    fn exported_constraints_are_the_same_constants_used_by_validation() {
        let constraints = ReaderPreferenceConstraints::default();
        assert_eq!(constraints.font_size_px, [FONT_SIZE_MIN, FONT_SIZE_MAX]);
        assert!(ReaderPreferences::default().set_font_size_px(constraints.font_size_px[0]).is_ok());
        assert!(ReaderPreferences::default().set_font_size_px(constraints.font_size_px[1]).is_ok());
        assert!(ReaderPreferences::default().set_font_size_px(constraints.font_size_px[1] + 1.0).is_err());
    }

    #[test]
    fn preference_options_cover_every_closed_enum_value_once() {
        let options = ReaderPreferenceOptions::default();
        assert_eq!(options.flows.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ReaderFlow::Paginated, ReaderFlow::Scrolled]);
        assert_eq!(options.font_families.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ReaderFontFamily::Publisher, ReaderFontFamily::Serif, ReaderFontFamily::SansSerif]);
        assert_eq!(options.text_alignments.iter().map(|option| option.value).collect::<Vec<_>>(), vec![ReaderTextAlignment::Publisher, ReaderTextAlignment::Start, ReaderTextAlignment::Justify]);
        assert_eq!(options.pdf_zoom_modes.iter().map(|option| option.value).collect::<Vec<_>>(), vec![PdfZoomMode::FitWidth, PdfZoomMode::Fixed, PdfZoomMode::FitHeight]);
    }

    #[test]
    fn pdf_layout_patches_are_persisted_by_the_shared_backend() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();

        let updated = context.apply_reader_preference_patch(ReaderPreferencePatch::PdfZoomMode(PdfZoomMode::FitHeight)).unwrap();
        assert_eq!(updated.pdf_zoom_mode(), PdfZoomMode::FitHeight);

        // Each mode keeps the value it holds fixed, so switching between them
        // restores what was set rather than starting over.
        context.apply_reader_preference_patch(ReaderPreferencePatch::PdfPageCount(4)).unwrap();
        let updated = context.apply_reader_preference_patch(ReaderPreferencePatch::PdfZoom(2.5)).unwrap();
        assert_eq!(updated.pdf_page_count(), 4);
        assert_eq!(updated.pdf_zoom(), 2.5);

        let reloaded = context.load_reader_preferences().unwrap();
        assert_eq!(reloaded.pdf_zoom_mode(), PdfZoomMode::FitHeight);
        assert_eq!(reloaded.pdf_page_count(), 4);
        assert_eq!(reloaded.pdf_zoom(), 2.5);
    }

    #[test]
    fn pdf_margin_trim_patch_is_persisted_by_the_shared_backend() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();

        let updated = context.apply_reader_preference_patch(ReaderPreferencePatch::PdfTrimMargins(false)).unwrap();

        assert!(!updated.pdf_trim_margins());
        assert!(!context.load_reader_preferences().unwrap().pdf_trim_margins());
    }
}
