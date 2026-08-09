//! Transport-neutral contracts shared by Bokheim frontends and application hosts.

pub mod account;
pub mod catalog;
pub mod download;
pub mod frontend;
pub mod library;
pub mod preferences;
pub mod reader_state;
pub mod semantic_links;
pub mod transfer;

pub use frontend::{
    resolve_reader_navigation_shortcut, AccountCapabilities, AccountContract, AccountValidationContract, AppFeature, BookActionsContract, BrowsingContentContract, BrowsingLayoutContract, BrowsingStatesContract, FeatureDefinition,
    FrontendContract, LibraryActionsContract, LibrarySyncStateDefinition, NetworkTransportDefinition, ReaderNavigationAction, ReaderNavigationContract, ReaderNavigationKey, ReaderNavigationShortcut, ReaderSettingsAnchor,
    ReaderSettingsPresentation, ReaderSettingsPresentationContract, SettingsContract, BOOK_IMPORT_EXTENSIONS, LARGE_BOOK_CARD_HEIGHT_PX, LARGE_BOOK_COVER_HEIGHT_PX, LARGE_BOOK_COVER_WIDTH_PX, LARGE_BOOK_GRID_COLUMN_GAP_PX,
    LARGE_BOOK_GRID_ROW_HEIGHT_PX, LARGE_CONTENT_PADDING_PX, LARGE_SIDEBAR_WIDTH_PX, LARGE_TOPBAR_ACTION_GAP_PX, LARGE_TOPBAR_ACTION_HEIGHT_PX, LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX, LARGE_TOPBAR_HEIGHT_PX,
    LARGE_VIEWPORT_BREAKPOINT_PX, READER_NAVIGATION_SHORTCUTS, READER_SETTINGS_OVERLAY_GAP_PX, READER_SETTINGS_OVERLAY_MAX_HEIGHT_PX, READER_SETTINGS_OVERLAY_MAX_WIDTH_PX, READER_SETTINGS_OVERLAY_MIN_COLUMN_WIDTH_PX,
    READER_SETTINGS_OVERLAY_PADDING_PX, SEMANTIC_LINK_RELATIONS,
};
