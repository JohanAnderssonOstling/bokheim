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
#[cfg(feature = "full")]
pub mod navigation;
pub mod preferences;
#[cfg(feature = "full")]
pub mod reader_state;
#[cfg(feature = "full")]
pub mod semantic_links;
#[cfg(feature = "full")]
pub mod transfer;

#[cfg(feature = "full")]
pub use navigation::{BrowserSession, LibraryPlace};

#[cfg(feature = "full")]
pub use frontend::{
    AUTHOR_ROW_CONTENT_GAP_PX, AUTHOR_ROW_COVER_GAP_PX, AUTHOR_ROW_HORIZONTAL_PADDING_PX, AUTHOR_ROW_LABEL_SIZE_PX, AccountCapabilities, AccountContract, AccountSummaryContract, AccountValidationContract, BOOK_IMPORT_EXTENSIONS,
    BookActionsContract, BrowsingContentContract, BrowsingLayoutContract, BrowsingStatesContract, DETAILED_ROW_ACTIONS_BREAKPOINT_PX, DETAILED_ROW_DESCRIPTION_LINES, DETAILED_ROW_HEIGHT_PX, DETAILED_ROW_TEXT_MEASURE_PX,
    DETAILED_ROW_THUMBNAIL_HEIGHT_PX, DETAILED_ROW_THUMBNAIL_WIDTH_PX, DETAILED_ROW_VERTICAL_PADDING_PX, FrontendContract, HIERARCHY_CELL_GAP_PX, HIERARCHY_CELL_HEIGHT_PX, HIERARCHY_CELL_MIN_WIDTH_PX, HIERARCHY_ICON_SIZE_PX,
    HIERARCHY_LABEL_SIZE_PX, HIERARCHY_TOUCH_CELL_MIN_HEIGHT_PX, LARGE_BOOK_CARD_HEIGHT_PX, LARGE_BOOK_COVER_HEIGHT_PX, LARGE_BOOK_COVER_WIDTH_PX, LARGE_BOOK_GRID_COLUMN_GAP_PX, LARGE_BOOK_GRID_ROW_HEIGHT_PX, LARGE_CONTENT_PADDING_PX,
    LARGE_SIDEBAR_WIDTH_PX, LARGE_TOPBAR_ACTION_GAP_PX, LARGE_TOPBAR_ACTION_HEIGHT_PX, LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX, LARGE_TOPBAR_HEIGHT_PX, LARGE_VIEWPORT_BREAKPOINT_PX, LIBRARY_FEATURES, LibraryActionsContract,
    LibraryFeature, LibraryFeatureDefinition, LibraryFileTypeFilterDefinition, LibrarySyncStateDefinition, METADATA_REVIEW_ITEM_HEIGHT_PX, NAVIGATION_ICON_LABEL_GAP_PX, NAVIGATION_ICON_SIZE_PX, NAVIGATION_ITEM_MIN_HEIGHT_PX,
    NAVIGATION_LABEL_SIZE_PX, NAVIGATION_LABEL_WEIGHT, NAVIGATION_TOUCH_ITEM_MIN_HEIGHT_PX, NavigationIcon, READER_NAVIGATION_SHORTCUTS, READER_SETTINGS_OVERLAY_GAP_PX, READER_SETTINGS_OVERLAY_MAX_HEIGHT_PX,
    READER_SETTINGS_OVERLAY_MAX_WIDTH_PX, READER_SETTINGS_OVERLAY_MIN_COLUMN_WIDTH_PX, READER_SETTINGS_OVERLAY_PADDING_PX, ReaderNavigationAction, ReaderNavigationContract, ReaderNavigationKey, ReaderNavigationShortcut,
    ReaderSettingsAnchor, ReaderSettingsPresentation, ReaderSettingsPresentationContract, SEMANTIC_LINK_RELATIONS, SURFACE_CORNER_RADIUS_PX, SettingsContract, TOPBAR_ACTION_LABEL_WEIGHT, UNIVERSAL_FEATURES, UniversalFeature,
    UniversalFeatureDefinition, resolve_reader_navigation_shortcut,
};
