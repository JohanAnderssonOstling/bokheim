//! Transport-neutral contracts shared by Bokheim frontends and application hosts.

#[cfg(feature = "full")]
pub mod account;
#[cfg(feature = "full")]
pub mod catalog;
#[cfg(feature = "full")]
pub mod design_tokens;
#[cfg(feature = "full")]
pub mod download;
#[cfg(feature = "full")]
pub mod frontend;
#[cfg(feature = "full")]
pub mod library;
pub mod preferences;
#[cfg(feature = "full")]
pub mod reader_state;
#[cfg(feature = "full")]
pub mod semantic_links;
#[cfg(feature = "full")]
pub mod transfer;

#[cfg(feature = "full")]
pub use frontend::{
    resolve_reader_navigation_shortcut, AccountCapabilities, AccountContract, AccountSummaryContract, AccountValidationContract, BookActionsContract, BrowsingContentContract, BrowsingLayoutContract, BrowsingStatesContract,
    FrontendContract, LibraryActionsContract, LibraryFeature, LibraryFeatureDefinition, LibrarySyncStateDefinition, NetworkTransportDefinition, ReaderNavigationAction, ReaderNavigationContract, ReaderNavigationKey,
    ReaderNavigationShortcut, ReaderSettingsAnchor, ReaderSettingsPresentation, ReaderSettingsPresentationContract, SettingsContract, UniversalFeature, UniversalFeatureDefinition, BOOK_IMPORT_EXTENSIONS, DETAILED_ROW_ACTIONS_BREAKPOINT_PX,
    DETAILED_ROW_DESCRIPTION_LINES, DETAILED_ROW_HEIGHT_PX, DETAILED_ROW_TEXT_MEASURE_PX, DETAILED_ROW_THUMBNAIL_HEIGHT_PX, DETAILED_ROW_THUMBNAIL_WIDTH_PX, DETAILED_ROW_VERTICAL_PADDING_PX, LARGE_BOOK_CARD_HEIGHT_PX,
    LARGE_BOOK_COVER_HEIGHT_PX, LARGE_BOOK_COVER_WIDTH_PX, LARGE_BOOK_GRID_COLUMN_GAP_PX, LARGE_BOOK_GRID_ROW_HEIGHT_PX, LARGE_CONTENT_PADDING_PX, LARGE_SIDEBAR_WIDTH_PX, LARGE_TOPBAR_ACTION_GAP_PX, LARGE_TOPBAR_ACTION_HEIGHT_PX,
    LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX, LARGE_TOPBAR_HEIGHT_PX, LARGE_VIEWPORT_BREAKPOINT_PX, LIBRARY_FEATURES, READER_NAVIGATION_SHORTCUTS, READER_SETTINGS_OVERLAY_GAP_PX, READER_SETTINGS_OVERLAY_MAX_HEIGHT_PX,
    READER_SETTINGS_OVERLAY_MAX_WIDTH_PX, READER_SETTINGS_OVERLAY_MIN_COLUMN_WIDTH_PX, READER_SETTINGS_OVERLAY_PADDING_PX, SEMANTIC_LINK_RELATIONS, UNIVERSAL_FEATURES,
};
