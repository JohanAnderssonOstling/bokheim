//! Shared presentation primitives for the GPUI desktop interface.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod annotation_editor;
pub use annotation_editor::{
    ANNOTATION_EDITOR_MAX_HEIGHT, ANNOTATION_EDITOR_WIDTH, AnnotationEditorTarget, EditorBook, RecurringEditorContext, annotation_editor_controls, annotation_editor_passage, annotation_note_input, recurring_annotation_editor_controls,
};

pub mod account;
pub mod assets;
pub mod audiobook;
pub mod book_overlay;
mod book_transfer_error;
pub mod browser;
pub mod browser_download;
pub mod browser_start;
mod button;
pub mod buttons;
pub mod contents;
pub mod detail;
mod errors;
mod fonts;
pub mod graph;
pub mod infinite_canvas;
pub mod keyboard;
pub mod keyboard_settings;
mod keyboard_shortcut_editor;
pub mod layout;
pub mod library;
mod menu_items;
pub mod modal;
#[cfg(test)]
mod overlay_tests;
pub mod player_dock;
pub mod reader;
pub mod responsive;
pub mod scroll;
pub mod settings_choice;
pub mod settings_stepper;
pub mod swipe;
pub mod theme;
pub mod theme_picker;
pub mod tokens;
mod touchpad;
pub mod trail;
pub mod trash;

pub use account::*;
pub use assets::*;
pub use audiobook::*;
pub use book_overlay::*;
pub use book_transfer_error::book_transfer_error_message;
pub use browser::*;
pub use browser_download::*;
pub use browser_start::*;
pub use button::Button;
pub use buttons::*;
pub use contents::*;
pub use detail::*;
pub use errors::{AccountAction, authentication_error, user_facing_error, wifi_error_message};
pub use fonts::{
    CJK_FONT_FAMILY, FontCatalog, FontCatalogTarget, SystemFontFamilies, bundled_font_loader, configured_ui_font_name, ensure_bundled_font, ensure_cjk_fonts, ensure_text_fonts, equal_choice_group_width, interface_label_width, load_font_catalog,
    observe_font_catalog, register_fonts, register_interface_fonts,
};
pub use graph::*;
pub use infinite_canvas::*;
pub use keyboard::*;
pub use keyboard_settings::*;
pub use keyboard_shortcut_editor::*;
pub use layout::*;
pub use library::*;
pub use menu_items::*;
pub use modal::*;
pub use operation_failure::{Classified, Failure, WifiFailure};
pub use player_dock::*;
pub use reader::*;
pub use responsive::*;
pub use scroll::*;
pub use settings_choice::*;
pub use settings_stepper::*;
pub use swipe::*;
pub use theme::*;
pub use theme_picker::*;
pub use tokens::*;
pub use touchpad::TouchpadNavigation;
pub use trail::*;
pub use trash::*;
#[cfg(feature = "kobo")]
pub mod kobo;

#[cfg(target_arch = "wasm32")]
pub use fonts::{InterfaceFontFace, interface_font_faces};
#[cfg(target_os = "android")]
pub use fonts::configure_bundled_font_source;
