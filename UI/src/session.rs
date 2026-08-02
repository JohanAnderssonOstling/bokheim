use crate::library_page::LibraryStateData;
use crate::page_navigation::ReaderState;
use epub_svelte_lib::app_core::AppCore;
use epub_svelte_lib::BookId;
use floem::kurbo::{Point, Size};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiSession {
    #[serde(default)]
    pub home: HomeSession,
    #[serde(default)]
    pub current_page: PersistedPage,
    #[serde(default)]
    pub reader_ui: ReaderUiSession,
    /// Read only to migrate pre-preferences versions of ui_session.json.
    #[serde(default, rename = "reader_settings", skip_serializing)]
    pub legacy_reader_settings: Option<LegacyReaderSettingsSession>,
    #[serde(default)]
    pub window: Option<WindowSession>,
}

impl Default for UiSession {
    fn default() -> Self {
        Self { home: HomeSession::default(), current_page: PersistedPage::Home, reader_ui: ReaderUiSession::default(), legacy_reader_settings: None, window: None }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HomeSession {
    pub active_library_id: Option<String>,
    #[serde(default)]
    pub libraries: Vec<LibraryStateData>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum PersistedPage {
    #[default]
    Home,
    Reader {
        library_id: String,
        book_id: BookId,
        book_title: String,
    },
}

impl From<&ReaderState> for PersistedPage {
    fn from(value: &ReaderState) -> Self {
        PersistedPage::Reader { library_id: value.library_id.clone(), book_id: value.book_id, book_title: value.book_title.clone() }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LegacyReaderSettingsSession {
    pub column_width: f64,
    pub font_size: f32,
    pub scale: f64,
    pub line_numbers: bool,
    #[serde(default = "default_left_panel_width")]
    pub left_panel_width: f64,
    #[serde(default = "default_toc_open")]
    pub toc_open: bool,
}

impl Default for LegacyReaderSettingsSession {
    fn default() -> Self {
        Self { column_width: 600.0, font_size: 20.0, scale: 1.0, line_numbers: true, left_panel_width: default_left_panel_width(), toc_open: default_toc_open() }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ReaderUiSession {
    #[serde(default = "default_left_panel_width")]
    pub left_panel_width: f64,
    #[serde(default = "default_toc_open")]
    pub toc_open: bool,
}

impl Default for ReaderUiSession {
    fn default() -> Self {
        Self { left_panel_width: default_left_panel_width(), toc_open: default_toc_open() }
    }
}

impl ReaderUiSession {
    pub fn sanitized(self) -> Self {
        Self { left_panel_width: if self.left_panel_width.is_finite() { self.left_panel_width.clamp(200.0, 560.0) } else { default_left_panel_width() }, toc_open: self.toc_open }
    }
}

fn default_left_panel_width() -> f64 {
    280.0
}

fn default_toc_open() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WindowSession {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

impl WindowSession {
    pub fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn position(self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn with_size(mut self, size: Size) -> Self {
        self.width = size.width.max(320.0);
        self.height = size.height.max(240.0);
        self
    }

    pub fn with_position(mut self, position: Point) -> Self {
        self.x = position.x;
        self.y = position.y;
        self
    }

    pub fn sanitized(self) -> Option<Self> {
        if self.width.is_finite() && self.height.is_finite() && self.x.is_finite() && self.y.is_finite() && self.width >= 320.0 && self.height >= 240.0 {
            Some(self)
        } else {
            None
        }
    }
}

pub fn session_path(core: &AppCore) -> PathBuf {
    core.app_data_dir().join("ui_session.json")
}

pub fn load_session(core: &AppCore) -> UiSession {
    let Ok(bytes) = fs::read(session_path(core)) else {
        return UiSession::default();
    };
    serde_json::from_slice::<UiSession>(&bytes).unwrap_or_default()
}

pub fn save_session(core: &AppCore, session: &UiSession) -> std::io::Result<()> {
    let path = session_path(core);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(session).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(path, bytes)
}
