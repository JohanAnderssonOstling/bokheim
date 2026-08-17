use crate::account::LibrarySyncState;
use crate::library::LibraryFileTypeFilter;
use crate::preferences::{BrowsingPreferencesContract, ReaderPreferencesContract, WebPreferenceProfilesContract};
use crate::transfer::NetworkTransport;
use account_contract::{EMAIL_MAX_BYTES, EMAIL_VERIFICATION_PIN_DIGITS, ONE_TIME_TOKEN_BYTES, PASSWORD_MAX_BYTES, PASSWORD_MIN_CHARACTERS};
use serde::{Deserialize, Serialize};
use specta::Type;

pub const BOOK_IMPORT_EXTENSIONS: [&str; 12] = ["epub", "pdf", "mobi", "azw", "azw3", "fb2", "cbz", "html", "htm", "xhtml", "webpub", "m4b"];
pub const SEMANTIC_LINK_RELATIONS: [&str; 8] = ["related to", "continues", "continued by", "inspired by", "adaptation of", "contrasts with", "commentary on", "translation of"];
pub const BREADCRUMB_ITEM_MAX_CHARACTERS: u8 = 24;

pub const LARGE_VIEWPORT_BREAKPOINT_PX: u16 = 720;
pub const LARGE_SIDEBAR_WIDTH_PX: u16 = 96;
pub const NAVIGATION_LABEL_SIZE_PX: u8 = 12;
pub const NAVIGATION_LABEL_WEIGHT: u16 = 600;
pub const NAVIGATION_ICON_SIZE_PX: u8 = 24;
pub const NAVIGATION_ICON_LABEL_GAP_PX: u8 = 5;
pub const NAVIGATION_ITEM_MIN_HEIGHT_PX: u8 = 64;
pub const NAVIGATION_TOUCH_ITEM_MIN_HEIGHT_PX: u8 = 56;
pub const LARGE_TOPBAR_HEIGHT_PX: u16 = 64;
pub const LARGE_TOPBAR_ACTION_HEIGHT_PX: u16 = 40;
pub const TOPBAR_ACTION_LABEL_WEIGHT: u16 = 600;
pub const LARGE_TOPBAR_ACTION_GAP_PX: u16 = 10;
pub const LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX: u16 = 11;
pub const LARGE_CONTENT_PADDING_PX: u16 = 18;
/// Shared interface surfaces and controls use square corners. Truly circular
/// status indicators may opt out because their shape carries meaning.
pub const SURFACE_CORNER_RADIUS_PX: u8 = 0;
/// Covers are 1:1.6, the ratio most trade publications sit closest to.
pub const LARGE_BOOK_COVER_WIDTH_PX: u16 = 200;
pub const LARGE_BOOK_COVER_HEIGHT_PX: u16 = 320;
/// The sum of the card's children, not a round number chosen independently:
/// 320 cover + 14 + 42 title + 5 + 22 author + 5 + 16 progress.
pub const LARGE_BOOK_CARD_HEIGHT_PX: u16 = 424;
pub const LARGE_BOOK_GRID_COLUMN_GAP_PX: u16 = 28;
pub const BOOK_CARD_TITLE_SIZE_PX: u8 = 17;
pub const BOOK_CARD_TITLE_WEIGHT: u16 = 600;
pub const BOOK_CARD_TITLE_LINE_HEIGHT_PX: u8 = 21;
pub const BOOK_CARD_TITLE_LINES: u8 = 2;
pub const BOOK_CARD_AUTHOR_SIZE_PX: u8 = 15;
pub const BOOK_CARD_AUTHOR_LINE_HEIGHT_PX: u8 = 22;
pub const BOOK_CARD_COVER_CONTENT_GAP_PX: u8 = 14;
pub const BOOK_CARD_TEXT_GAP_PX: u8 = 5;
pub const BOOK_CARD_PROGRESS_HEIGHT_PX: u8 = 16;
pub const BOOK_GRID_ROW_TRAILING_SPACE_PX: u8 = 6;
pub const CAROUSEL_TITLE_SIZE_PX: u8 = 21;
pub const CAROUSEL_TITLE_WEIGHT: u16 = 600;
pub const CAROUSEL_SECTION_GAP_PX: u8 = 28;
pub const CAROUSEL_CONTENT_GAP_PX: u8 = 14;
pub const CAROUSEL_CONTROL_GAP_PX: u8 = 5;
pub const CAROUSEL_ARROW_SIZE_PX: u8 = 34;

/// Author cards reuse the regular book-cover geometry. Their width is derived
/// from the one-to-three books they contain, while every card keeps the same
/// height: 14 padding + 42 name + 14 gap + 320 covers + 14 padding.
pub const AUTHOR_ROW_COVER_COUNT: u8 = 3;
pub const AUTHOR_ROW_COVER_WIDTH_PX: u16 = LARGE_BOOK_COVER_WIDTH_PX;
pub const AUTHOR_ROW_COVER_HEIGHT_PX: u16 = LARGE_BOOK_COVER_HEIGHT_PX;
pub const AUTHOR_ROW_HEIGHT_PX: u16 = 404;
pub const AUTHOR_ROW_COVER_GAP_PX: u8 = 14;
pub const AUTHOR_ROW_CONTENT_GAP_PX: u8 = 14;
pub const AUTHOR_ROW_HORIZONTAL_PADDING_PX: u8 = 14;
pub const AUTHOR_ROW_LABEL_SIZE_PX: u8 = BOOK_CARD_TITLE_SIZE_PX;
pub const LARGE_BOOK_GRID_ROW_HEIGHT_PX: u16 = LARGE_BOOK_CARD_HEIGHT_PX + BOOK_GRID_ROW_TRAILING_SPACE_PX as u16;

/// Folder and subject navigation share one visual geometry across frontends.
/// Pointer-first layouts keep the compact row; touch-first layouts use the
/// larger minimum without changing the label, icon, or grid rhythm.
pub const HIERARCHY_CELL_HEIGHT_PX: u16 = 40;
pub const HIERARCHY_TOUCH_CELL_MIN_HEIGHT_PX: u16 = 56;
pub const HIERARCHY_CELL_MIN_WIDTH_PX: u16 = 240;
pub const HIERARCHY_CELL_GAP_PX: u8 = 4;
pub const HIERARCHY_ICON_SIZE_PX: u8 = 22;
pub const HIERARCHY_LABEL_SIZE_PX: u8 = 18;

/// The detailed book row — cover art beside the text, rather than under it.
///
/// The thumbnail keeps the 1:1.6 cover ratio and is sized to the text block, so
/// nothing dangles: 22 title + 5 + 20 author + 5 + 84 description (four lines)
/// + 5 + 16 progress is 157, against a 160 thumbnail.
pub const DETAILED_ROW_THUMBNAIL_WIDTH_PX: u16 = 100;
pub const DETAILED_ROW_THUMBNAIL_HEIGHT_PX: u16 = 160;
/// Derived rather than stated, so the row cannot end up shorter than the art
/// it contains — which is exactly how the cover card acquired 22px of slack
/// nobody could account for.
pub const DETAILED_ROW_VERTICAL_PADDING_PX: u16 = 9;
pub const DETAILED_ROW_HEIGHT_PX: u16 = DETAILED_ROW_THUMBNAIL_HEIGHT_PX + DETAILED_ROW_VERTICAL_PADDING_PX * 2;
/// Descriptions are capped to a readable measure rather than running the full
/// window width: at 15px this is roughly 70 characters, and the four-line clamp
/// means the row height stays fixed however wide the window is.
pub const DETAILED_ROW_TEXT_MEASURE_PX: u16 = 640;
pub const DETAILED_ROW_DESCRIPTION_LINES: u8 = 4;
/// Content width at which a detailed row has room for its action buttons
/// beside the text instead of below it. Phones never reach this, including in
/// landscape, which is the correct outcome there.
pub const DETAILED_ROW_ACTIONS_BREAKPOINT_PX: u16 = 940;
/// Fixed height for every item in the three metadata-review modes. Keeping
/// ambiguous, no-match, and failed items identical prevents the virtual lists
/// in the two frontends from changing density when a status tab is selected.
pub const METADATA_REVIEW_ITEM_HEIGHT_PX: u16 = DETAILED_ROW_HEIGHT_PX * 2 + 20;
pub const STORE_MAX_WIDTH_PX: u16 = 1248;
pub const STORE_CATALOG_TWO_COLUMN_BREAKPOINT_PX: u16 = 720;
pub const STORE_CATALOG_THREE_COLUMN_BREAKPOINT_PX: u16 = 1152;
pub const STORE_PUBLICATION_MIN_WIDTH_PX: u16 = 400;
pub const STORE_PUBLICATION_TWO_COLUMN_BREAKPOINT_PX: u16 = 816;
pub const STORE_PUBLICATION_THREE_COLUMN_BREAKPOINT_PX: u16 = 1232;
pub const STORE_DESCRIPTION_SCROLLBAR_WIDTH_PX: u8 = 8;
pub const READER_SETTINGS_OVERLAY_MAX_WIDTH_PX: u16 = 400;
pub const READER_SETTINGS_OVERLAY_MAX_HEIGHT_PX: u16 = 620;
pub const READER_SETTINGS_OVERLAY_PADDING_PX: u8 = 16;
pub const READER_SETTINGS_OVERLAY_GAP_PX: u8 = 13;
pub const READER_SETTINGS_OVERLAY_MIN_COLUMN_WIDTH_PX: u16 = 176;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
pub struct BrowsingLayoutContract {
    pub large_viewport_breakpoint_px: u16,
    pub sidebar_width_px: u16,
    pub navigation_label_size_px: u8,
    pub navigation_label_weight: u16,
    pub navigation_icon_size_px: u8,
    pub navigation_icon_label_gap_px: u8,
    pub navigation_item_min_height_px: u8,
    pub navigation_touch_item_min_height_px: u8,
    pub topbar_height_px: u16,
    pub topbar_action_height_px: u16,
    pub topbar_action_label_weight: u16,
    pub topbar_action_gap_px: u16,
    pub topbar_action_horizontal_padding_px: u16,
    pub content_padding_px: u16,
    pub surface_corner_radius_px: u8,
    pub book_cover_width_px: u16,
    pub book_cover_height_px: u16,
    pub book_card_height_px: u16,
    pub book_grid_column_gap_px: u16,
    pub book_grid_row_height_px: u16,
    pub book_card_title_size_px: u8,
    pub book_card_title_weight: u16,
    pub book_card_title_line_height_px: u8,
    pub book_card_title_lines: u8,
    pub book_card_author_size_px: u8,
    pub book_card_author_line_height_px: u8,
    pub book_card_cover_content_gap_px: u8,
    pub book_card_text_gap_px: u8,
    pub book_card_progress_height_px: u8,
    pub book_grid_row_trailing_space_px: u8,
    pub carousel_title_size_px: u8,
    pub carousel_title_weight: u16,
    pub carousel_section_gap_px: u8,
    pub carousel_content_gap_px: u8,
    pub carousel_control_gap_px: u8,
    pub carousel_arrow_size_px: u8,
    pub detailed_row_thumbnail_width_px: u16,
    pub detailed_row_thumbnail_height_px: u16,
    pub detailed_row_height_px: u16,
    pub detailed_row_text_measure_px: u16,
    pub detailed_row_description_lines: u8,
    pub detailed_row_actions_breakpoint_px: u16,
    pub author_row_cover_count: u8,
    pub author_row_cover_width_px: u16,
    pub author_row_cover_height_px: u16,
    pub author_row_height_px: u16,
    pub author_row_cover_gap_px: u8,
    pub author_row_content_gap_px: u8,
    pub author_row_horizontal_padding_px: u8,
    pub author_row_label_size_px: u8,
    pub hierarchy_cell_height_px: u16,
    pub hierarchy_touch_cell_min_height_px: u16,
    pub hierarchy_cell_min_width_px: u16,
    pub hierarchy_cell_gap_px: u8,
    pub hierarchy_icon_size_px: u8,
    pub hierarchy_label_size_px: u8,
}

impl Default for BrowsingLayoutContract {
    fn default() -> Self {
        Self {
            large_viewport_breakpoint_px: LARGE_VIEWPORT_BREAKPOINT_PX,
            sidebar_width_px: LARGE_SIDEBAR_WIDTH_PX,
            navigation_label_size_px: NAVIGATION_LABEL_SIZE_PX,
            navigation_label_weight: NAVIGATION_LABEL_WEIGHT,
            navigation_icon_size_px: NAVIGATION_ICON_SIZE_PX,
            navigation_icon_label_gap_px: NAVIGATION_ICON_LABEL_GAP_PX,
            navigation_item_min_height_px: NAVIGATION_ITEM_MIN_HEIGHT_PX,
            navigation_touch_item_min_height_px: NAVIGATION_TOUCH_ITEM_MIN_HEIGHT_PX,
            topbar_height_px: LARGE_TOPBAR_HEIGHT_PX,
            topbar_action_height_px: LARGE_TOPBAR_ACTION_HEIGHT_PX,
            topbar_action_label_weight: TOPBAR_ACTION_LABEL_WEIGHT,
            topbar_action_gap_px: LARGE_TOPBAR_ACTION_GAP_PX,
            topbar_action_horizontal_padding_px: LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX,
            content_padding_px: LARGE_CONTENT_PADDING_PX,
            surface_corner_radius_px: SURFACE_CORNER_RADIUS_PX,
            book_cover_width_px: LARGE_BOOK_COVER_WIDTH_PX,
            book_cover_height_px: LARGE_BOOK_COVER_HEIGHT_PX,
            book_card_height_px: LARGE_BOOK_CARD_HEIGHT_PX,
            book_grid_column_gap_px: LARGE_BOOK_GRID_COLUMN_GAP_PX,
            book_grid_row_height_px: LARGE_BOOK_GRID_ROW_HEIGHT_PX,
            book_card_title_size_px: BOOK_CARD_TITLE_SIZE_PX,
            book_card_title_weight: BOOK_CARD_TITLE_WEIGHT,
            book_card_title_line_height_px: BOOK_CARD_TITLE_LINE_HEIGHT_PX,
            book_card_title_lines: BOOK_CARD_TITLE_LINES,
            book_card_author_size_px: BOOK_CARD_AUTHOR_SIZE_PX,
            book_card_author_line_height_px: BOOK_CARD_AUTHOR_LINE_HEIGHT_PX,
            book_card_cover_content_gap_px: BOOK_CARD_COVER_CONTENT_GAP_PX,
            book_card_text_gap_px: BOOK_CARD_TEXT_GAP_PX,
            book_card_progress_height_px: BOOK_CARD_PROGRESS_HEIGHT_PX,
            book_grid_row_trailing_space_px: BOOK_GRID_ROW_TRAILING_SPACE_PX,
            carousel_title_size_px: CAROUSEL_TITLE_SIZE_PX,
            carousel_title_weight: CAROUSEL_TITLE_WEIGHT,
            carousel_section_gap_px: CAROUSEL_SECTION_GAP_PX,
            carousel_content_gap_px: CAROUSEL_CONTENT_GAP_PX,
            carousel_control_gap_px: CAROUSEL_CONTROL_GAP_PX,
            carousel_arrow_size_px: CAROUSEL_ARROW_SIZE_PX,
            detailed_row_thumbnail_width_px: DETAILED_ROW_THUMBNAIL_WIDTH_PX,
            detailed_row_thumbnail_height_px: DETAILED_ROW_THUMBNAIL_HEIGHT_PX,
            detailed_row_height_px: DETAILED_ROW_HEIGHT_PX,
            detailed_row_text_measure_px: DETAILED_ROW_TEXT_MEASURE_PX,
            detailed_row_description_lines: DETAILED_ROW_DESCRIPTION_LINES,
            detailed_row_actions_breakpoint_px: DETAILED_ROW_ACTIONS_BREAKPOINT_PX,
            author_row_cover_count: AUTHOR_ROW_COVER_COUNT,
            author_row_cover_width_px: AUTHOR_ROW_COVER_WIDTH_PX,
            author_row_cover_height_px: AUTHOR_ROW_COVER_HEIGHT_PX,
            author_row_height_px: AUTHOR_ROW_HEIGHT_PX,
            author_row_cover_gap_px: AUTHOR_ROW_COVER_GAP_PX,
            author_row_content_gap_px: AUTHOR_ROW_CONTENT_GAP_PX,
            author_row_horizontal_padding_px: AUTHOR_ROW_HORIZONTAL_PADDING_PX,
            author_row_label_size_px: AUTHOR_ROW_LABEL_SIZE_PX,
            hierarchy_cell_height_px: HIERARCHY_CELL_HEIGHT_PX,
            hierarchy_touch_cell_min_height_px: HIERARCHY_TOUCH_CELL_MIN_HEIGHT_PX,
            hierarchy_cell_min_width_px: HIERARCHY_CELL_MIN_WIDTH_PX,
            hierarchy_cell_gap_px: HIERARCHY_CELL_GAP_PX,
            hierarchy_icon_size_px: HIERARCHY_ICON_SIZE_PX,
            hierarchy_label_size_px: HIERARCHY_LABEL_SIZE_PX,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderSettingsPresentation {
    ContextualOverlay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderSettingsAnchor {
    ReaderToolbarEnd,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct ReaderSettingsPresentationContract {
    pub presentation: ReaderSettingsPresentation,
    pub anchor: ReaderSettingsAnchor,
    pub trigger_label: String,
    pub title: String,
    pub max_width_px: u16,
    pub max_height_px: u16,
    pub padding_px: u8,
    pub gap_px: u8,
    pub minimum_column_width_px: u16,
    pub keeps_document_visible: bool,
}

impl Default for ReaderSettingsPresentationContract {
    fn default() -> Self {
        Self {
            presentation: ReaderSettingsPresentation::ContextualOverlay,
            anchor: ReaderSettingsAnchor::ReaderToolbarEnd,
            trigger_label: "Aa".to_owned(),
            title: "Appearance".to_owned(),
            max_width_px: READER_SETTINGS_OVERLAY_MAX_WIDTH_PX,
            max_height_px: READER_SETTINGS_OVERLAY_MAX_HEIGHT_PX,
            padding_px: READER_SETTINGS_OVERLAY_PADDING_PX,
            gap_px: READER_SETTINGS_OVERLAY_GAP_PX,
            minimum_column_width_px: READER_SETTINGS_OVERLAY_MIN_COLUMN_WIDTH_PX,
            keeps_document_visible: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct LibraryActionsContract {
    pub add_book_label: String,
    pub new_folder_label: String,
    pub download_folder_label: String,
    pub folder_downloaded_label: String,
    pub show_bulk_download_in_topbar: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct StoreContract {
    pub max_width_px: u16,
    pub catalog_two_column_breakpoint_px: u16,
    pub catalog_three_column_breakpoint_px: u16,
    pub publication_min_width_px: u16,
    pub publication_two_column_breakpoint_px: u16,
    pub publication_three_column_breakpoint_px: u16,
    pub publication_height_px: u16,
    pub description_scrollbar_width_px: u8,
    pub catalogs_label: String,
    pub search_all_placeholder: String,
    pub search_label: String,
    pub add_label: String,
    pub adding_label: String,
    pub open_label: String,
    pub retry_label: String,
    pub sign_in_needed_label: String,
}

impl Default for StoreContract {
    fn default() -> Self {
        Self {
            max_width_px: STORE_MAX_WIDTH_PX,
            catalog_two_column_breakpoint_px: STORE_CATALOG_TWO_COLUMN_BREAKPOINT_PX,
            catalog_three_column_breakpoint_px: STORE_CATALOG_THREE_COLUMN_BREAKPOINT_PX,
            publication_min_width_px: STORE_PUBLICATION_MIN_WIDTH_PX,
            publication_two_column_breakpoint_px: STORE_PUBLICATION_TWO_COLUMN_BREAKPOINT_PX,
            publication_three_column_breakpoint_px: STORE_PUBLICATION_THREE_COLUMN_BREAKPOINT_PX,
            publication_height_px: LARGE_BOOK_CARD_HEIGHT_PX,
            description_scrollbar_width_px: STORE_DESCRIPTION_SCROLLBAR_WIDTH_PX,
            catalogs_label: "Catalogs".to_owned(),
            search_all_placeholder: "Search all catalogs".to_owned(),
            search_label: "Search".to_owned(),
            add_label: "Add".to_owned(),
            adding_label: "Adding…".to_owned(),
            open_label: "Open".to_owned(),
            retry_label: "Retry".to_owned(),
            sign_in_needed_label: "Sign in needed".to_owned(),
        }
    }
}

impl Default for LibraryActionsContract {
    fn default() -> Self {
        Self { add_book_label: "Add book".to_owned(), new_folder_label: "New folder".to_owned(), download_folder_label: "Download folder".to_owned(), folder_downloaded_label: "Downloaded".to_owned(), show_bulk_download_in_topbar: false }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BrowsingStatesContract {
    pub loading_library_label: String,
    pub loading_folder_label: String,
    pub loading_books_label: String,
    pub loading_authors_label: String,
    pub searching_label: String,
    pub no_search_results_label: String,
    pub load_error_title: String,
    pub no_libraries_title: String,
    pub no_libraries_description: String,
    pub create_library_label: String,
    pub empty_library_title: String,
    pub empty_library_description: String,
    pub empty_folder_title: String,
    pub empty_folder_description: String,
    pub retry_label: String,
}

impl Default for BrowsingStatesContract {
    fn default() -> Self {
        Self {
            loading_library_label: "Loading library…".to_owned(),
            loading_folder_label: "Loading folder…".to_owned(),
            loading_books_label: "Loading books…".to_owned(),
            loading_authors_label: "Loading authors…".to_owned(),
            searching_label: "Searching…".to_owned(),
            no_search_results_label: "No matching books".to_owned(),
            load_error_title: "Could not load this section".to_owned(),
            no_libraries_title: "No libraries".to_owned(),
            no_libraries_description: "Create a library to organize your books.".to_owned(),
            create_library_label: "Create library".to_owned(),
            empty_library_title: "Your library is empty".to_owned(),
            empty_library_description: "Add a book or create a folder to get started.".to_owned(),
            empty_folder_title: "This folder is empty".to_owned(),
            empty_folder_description: "Add a book or create a folder here.".to_owned(),
            retry_label: "Retry".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BrowsingContentContract {
    pub minimum_search_characters: u8,
    pub search_debounce_ms: u16,
    pub breadcrumb_item_max_characters: u8,
    pub search_result_limit: u16,
    pub home_book_limit: u8,
    pub finished_progress_threshold: u8,
    pub import_extensions: Vec<String>,
    pub file_type_filters: Vec<LibraryFileTypeFilterDefinition>,
    pub semantic_link_relations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct LibraryFileTypeFilterDefinition {
    pub value: LibraryFileTypeFilter,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BookActionsContract {
    pub menu_label: String,
    pub open_label: String,
    pub download_label: String,
    pub download_in_progress_label: String,
    pub remove_local_copy_label: String,
    pub hide_label: String,
    pub restore_label: String,
}

impl Default for BookActionsContract {
    fn default() -> Self {
        Self {
            menu_label: "Book actions".to_owned(),
            open_label: "Open".to_owned(),
            download_label: "Download".to_owned(),
            download_in_progress_label: "Download in progress".to_owned(),
            remove_local_copy_label: "Remove local copy".to_owned(),
            hide_label: "Hide from library".to_owned(),
            restore_label: "Add back to library".to_owned(),
        }
    }
}

impl Default for BrowsingContentContract {
    fn default() -> Self {
        Self {
            minimum_search_characters: 1,
            search_debounce_ms: 220,
            breadcrumb_item_max_characters: BREADCRUMB_ITEM_MAX_CHARACTERS,
            search_result_limit: 100,
            home_book_limit: 20,
            finished_progress_threshold: 98,
            import_extensions: BOOK_IMPORT_EXTENSIONS.into_iter().map(str::to_owned).collect(),
            file_type_filters: LibraryFileTypeFilter::ALL.into_iter().map(|value| LibraryFileTypeFilterDefinition { value, label: value.label().to_owned() }).collect(),
            semantic_link_relations: SEMANTIC_LINK_RELATIONS.into_iter().map(str::to_owned).collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AccountCapabilities {
    pub public_registration: bool,
    pub email_verification: bool,
    pub resend_verification: bool,
    pub password_recovery: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AccountValidationContract {
    pub password_min_characters: u16,
    pub password_max_bytes: u16,
    pub email_max_bytes: u16,
    pub verification_pin_digits: u16,
    pub one_time_token_bytes: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct LibrarySyncStateDefinition {
    pub state: LibrarySyncState,
    pub label: String,
    pub description: String,
}

/// Copy for the verdict the account page opens with, and for the storage meter
/// that explains the quota underneath it.
///
/// Grouped rather than flattened into [`AccountContract`] because these strings
/// only make sense together: they are the one summary both frontends render
/// from [`crate::account::LibrarySyncSummary`], and they must not drift apart.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AccountSummaryContract {
    pub all_synced_label: String,
    pub attention_label: String,
    /// Completes "3 of 5 libraries …" in the line under the headline.
    pub needs_attention_suffix: String,
    pub syncing_label: String,
    pub sync_now_label: String,
    pub storage_section_label: String,
    /// Charged bytes belonging to no library this device knows about: a library
    /// on the account that was never enrolled here, or blobs still charged
    /// after being released everywhere.
    pub unattributed_label: String,
    /// Heads the column showing what each library occupies on the server.
    pub server_usage_label: String,
    /// Heads the column showing what has been downloaded to this device.
    pub device_usage_label: String,
    /// Shown instead of a breakdown when the server predates the per-library
    /// storage endpoint.
    pub breakdown_unavailable_label: String,
    pub reserved_label: String,
    pub other_libraries_label: String,
    /// Completes "12 batch jobs …" in the activity footer.
    pub finished_suffix: String,
    /// One line standing in for the batch job sections while they are empty.
    pub activity_idle_label: String,
    pub show_activity_label: String,
    pub hide_activity_label: String,
}

impl Default for AccountSummaryContract {
    fn default() -> Self {
        Self {
            all_synced_label: "Everything is synced".to_owned(),
            attention_label: "Needs attention".to_owned(),
            needs_attention_suffix: "need attention".to_owned(),
            syncing_label: "Syncing…".to_owned(),
            sync_now_label: "Sync now".to_owned(),
            storage_section_label: "Server storage".to_owned(),
            unattributed_label: "Elsewhere on the account".to_owned(),
            server_usage_label: "On the server".to_owned(),
            device_usage_label: "On this device".to_owned(),
            breakdown_unavailable_label: "This server does not report storage per library.".to_owned(),
            reserved_label: "Reserved by uploads".to_owned(),
            other_libraries_label: "Other libraries".to_owned(),
            finished_suffix: "finished".to_owned(),
            activity_idle_label: "Nothing queued".to_owned(),
            show_activity_label: "Show batch jobs".to_owned(),
            hide_activity_label: "Hide batch jobs".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AccountContract {
    pub navigation_label: String,
    pub title: String,
    pub libraries_section_label: String,
    pub transfers_section_label: String,
    pub transfer_history_section_label: String,
    pub no_local_libraries_label: String,
    pub no_active_transfers_label: String,
    pub no_transfer_history_label: String,
    pub refresh_label: String,
    pub sign_out_label: String,
    pub summary: AccountSummaryContract,
    pub capabilities: AccountCapabilities,
    pub validation: AccountValidationContract,
    pub sync_states: Vec<LibrarySyncStateDefinition>,
}

impl Default for AccountContract {
    fn default() -> Self {
        let sync_states = [
            (LibrarySyncState::LocalOnly, "Local", "Stored only on this device"),
            (LibrarySyncState::Synced, "Synced", "Available on this account"),
            (LibrarySyncState::SignInRequired, "Sign in required", "Sign in to resume synchronization"),
            (LibrarySyncState::DifferentAccount, "Unavailable", "Unavailable with the current sign-in"),
            (LibrarySyncState::RemoteMissing, "Upload pending", "Ready to create on the server"),
        ]
        .into_iter()
        .map(|(state, label, description)| LibrarySyncStateDefinition { state, label: label.to_owned(), description: description.to_owned() })
        .collect();
        Self {
            navigation_label: "Account".to_owned(),
            title: "Account".to_owned(),
            libraries_section_label: "Libraries on this device".to_owned(),
            transfers_section_label: "Batch jobs".to_owned(),
            transfer_history_section_label: "Batch job history".to_owned(),
            no_local_libraries_label: "No local libraries".to_owned(),
            no_active_transfers_label: "No active batch jobs".to_owned(),
            no_transfer_history_label: "No completed batch jobs".to_owned(),
            refresh_label: "Refresh".to_owned(),
            sign_out_label: "Sign out".to_owned(),
            summary: AccountSummaryContract::default(),
            capabilities: AccountCapabilities { public_registration: true, email_verification: true, resend_verification: true, password_recovery: true },
            validation: AccountValidationContract {
                password_min_characters: PASSWORD_MIN_CHARACTERS as u16,
                password_max_bytes: PASSWORD_MAX_BYTES as u16,
                email_max_bytes: EMAIL_MAX_BYTES as u16,
                verification_pin_digits: EMAIL_VERIFICATION_PIN_DIGITS as u16,
                one_time_token_bytes: ONE_TIME_TOKEN_BYTES as u16,
            },
            sync_states,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct NetworkTransportDefinition {
    pub transport: NetworkTransport,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct SettingsContract {
    pub navigation_label: String,
    pub title: String,
    pub appearance_section_label: String,
    pub transfers_section_label: String,
    pub catalogues_section_label: String,
    pub included_catalog_languages_label: String,
    pub included_catalog_languages_description: String,
    pub all_catalog_languages_label: String,
    pub application_theme_label: String,
    pub interface_font_label: String,
    pub ui_font_size_label: String,
    pub library_section_label: String,
    pub library_view_label: String,
    pub show_unavailable_books_label: String,
    pub background_metered_label: String,
    pub background_metered_description: String,
    pub auto_download_label: String,
    pub auto_download_description: String,
    pub current_network_label: String,
    pub network_transports: Vec<NetworkTransportDefinition>,
}

impl Default for SettingsContract {
    fn default() -> Self {
        let network_transports = [
            (NetworkTransport::Offline, "Offline"),
            (NetworkTransport::Wifi, "Wi-Fi"),
            (NetworkTransport::Cellular, "Mobile data"),
            (NetworkTransport::Ethernet, "Ethernet"),
            (NetworkTransport::Vpn, "VPN"),
            (NetworkTransport::Other, "Connected"),
            (NetworkTransport::Unknown, "Network unknown"),
        ]
        .into_iter()
        .map(|(transport, label)| NetworkTransportDefinition { transport, label: label.to_owned() })
        .collect();
        Self {
            navigation_label: "Settings".to_owned(),
            title: "Settings".to_owned(),
            appearance_section_label: "Appearance".to_owned(),
            transfers_section_label: "Batch jobs".to_owned(),
            catalogues_section_label: "Catalogues".to_owned(),
            included_catalog_languages_label: "Included languages".to_owned(),
            included_catalog_languages_description: "Limits catalogue-wide search. Catalogue providers remain available in the Store.".to_owned(),
            all_catalog_languages_label: "All languages".to_owned(),
            application_theme_label: "Application theme".to_owned(),
            interface_font_label: "Interface font".to_owned(),
            ui_font_size_label: "UI font size".to_owned(),
            library_section_label: "Library".to_owned(),
            library_view_label: "Book display".to_owned(),
            show_unavailable_books_label: "Show books not on this device".to_owned(),
            background_metered_label: "Background transfers on metered connections".to_owned(),
            background_metered_description: "Manual uploads and downloads always continue.".to_owned(),
            auto_download_label: "Automatic book downloads".to_owned(),
            auto_download_description: "Download server books up to this size on this device.".to_owned(),
            current_network_label: "Current network".to_owned(),
            network_transports,
        }
    }
}

/// A destination whose contents belong to the selected library.
///
/// Split from [`UniversalFeature`] rather than tagging one flat list with a
/// scope, so that a library-scoped destination cannot be listed among the
/// app-scoped ones by mistake. This mirrors the `LibraryRoute`/`UniversalRoute`
/// split the desktop frontend already makes one layer down.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryFeature {
    Home,
    /// Named for what it shows. It leads to the folder hierarchy, and "Library"
    /// would collide with the *selected library* named beside it in the rail.
    Folders,
    Subjects,
    Authors,
    Trash,
}

/// Semantic navigation artwork shared by every frontend. Renderers translate
/// these names into their native icon implementation; the contract owns the
/// choice so desktop and mobile cannot assign different symbols to one route.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NavigationIcon {
    House,
    BookOpen,
    Folder,
    Asterisk,
    Tags,
    User,
    Trash2,
    Building2,
    CircleUser,
    Settings,
}

impl LibraryFeature {
    pub const ALL: &'static [Self] = &LIBRARY_FEATURES;

    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Folders => "Folders",
            Self::Subjects => "Subjects",
            Self::Authors => "Authors",
            Self::Trash => "Trash",
        }
    }

    pub const fn icon(self) -> NavigationIcon {
        match self {
            Self::Home => NavigationIcon::House,
            Self::Folders => NavigationIcon::Folder,
            Self::Subjects => NavigationIcon::Tags,
            Self::Authors => NavigationIcon::User,
            Self::Trash => NavigationIcon::Trash2,
        }
    }
}

impl Default for LibraryFeature {
    fn default() -> Self {
        Self::Folders
    }
}

/// A destination that outlives the selected library.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UniversalFeature {
    Store,
    Account,
    Settings,
}

impl UniversalFeature {
    pub const ALL: &'static [Self] = &UNIVERSAL_FEATURES;

    pub const fn label(self) -> &'static str {
        match self {
            Self::Store => "Store",
            Self::Account => "Account",
            Self::Settings => "Settings",
        }
    }

    pub const fn icon(self) -> NavigationIcon {
        match self {
            Self::Store => NavigationIcon::Building2,
            Self::Account => NavigationIcon::CircleUser,
            Self::Settings => NavigationIcon::Settings,
        }
    }
}

impl Default for UniversalFeature {
    fn default() -> Self {
        Self::Store
    }
}

/// The order destinations appear in navigation, and the order their numeric
/// shortcuts follow. Frontends render these rather than keeping their own
/// lists, so a destination cannot exist on one platform without the others
/// accounting for it — which is why Account and Settings are here rather than
/// being invented separately by each frontend, as they were before.
///
/// The two lists are drawn as two groups with a divider between them: the
/// boundary between "this library" and "this app" is a real distinction and the
/// navigation should show it.
/// Search is deliberately absent: the folder and subject pages search every
/// book beneath the node being browsed, and at the root of either that scope is
/// the whole library — which is all a separate Search destination ever offered.
pub const LIBRARY_FEATURES: [LibraryFeature; 5] = [LibraryFeature::Home, LibraryFeature::Folders, LibraryFeature::Subjects, LibraryFeature::Authors, LibraryFeature::Trash];
pub const UNIVERSAL_FEATURES: [UniversalFeature; 3] = [UniversalFeature::Store, UniversalFeature::Account, UniversalFeature::Settings];

/// Cross-frontend browser-session behavior. The serialized representation is
/// platform-owned, while these rules are shared so switching frontends does
/// not change what "resume browsing" means.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BrowserSessionContract {
    pub default_library_route: LibraryFeature,
    pub restore_places_per_library: bool,
    pub persist_universal_routes: bool,
}

impl Default for BrowserSessionContract {
    fn default() -> Self {
        Self { default_library_route: LibraryFeature::Folders, restore_places_per_library: true, persist_universal_routes: false }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct LibraryFeatureDefinition {
    pub id: LibraryFeature,
    pub label: String,
    pub icon: NavigationIcon,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct UniversalFeatureDefinition {
    pub id: UniversalFeature,
    pub label: String,
    pub icon: NavigationIcon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderNavigationKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Space,
}

impl ReaderNavigationKey {
    /// Converts the key names used by both DOM `KeyboardEvent` and GPUI.
    pub fn from_key_name(key: &str) -> Option<Self> {
        match key {
            "ArrowUp" | "up" => Some(Self::ArrowUp),
            "ArrowDown" | "down" => Some(Self::ArrowDown),
            "ArrowLeft" | "left" => Some(Self::ArrowLeft),
            "ArrowRight" | "right" => Some(Self::ArrowRight),
            "PageUp" | "pageup" => Some(Self::PageUp),
            "PageDown" | "pagedown" => Some(Self::PageDown),
            " " | "Space" | "space" => Some(Self::Space),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReaderNavigationAction {
    PreviousLine,
    NextLine,
    PreviousPage,
    NextPage,
    PreviousSection,
    NextSection,
    HistoryBack,
    HistoryForward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct ReaderNavigationShortcut {
    pub key: ReaderNavigationKey,
    /// Ctrl on Linux/Windows and Cmd on macOS or the web.
    pub primary_modifier: bool,
    pub alt: bool,
    pub shift: bool,
    pub action: ReaderNavigationAction,
}

impl ReaderNavigationShortcut {
    pub const fn new(key: ReaderNavigationKey, primary_modifier: bool, alt: bool, shift: bool, action: ReaderNavigationAction) -> Self {
        Self { key, primary_modifier, alt, shift, action }
    }
}

pub const READER_NAVIGATION_SHORTCUTS: [ReaderNavigationShortcut; 11] = [
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowUp, false, false, false, ReaderNavigationAction::PreviousLine),
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowDown, false, false, false, ReaderNavigationAction::NextLine),
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowLeft, false, false, false, ReaderNavigationAction::PreviousPage),
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowRight, false, false, false, ReaderNavigationAction::NextPage),
    ReaderNavigationShortcut::new(ReaderNavigationKey::PageUp, false, false, false, ReaderNavigationAction::PreviousPage),
    ReaderNavigationShortcut::new(ReaderNavigationKey::PageDown, false, false, false, ReaderNavigationAction::NextPage),
    ReaderNavigationShortcut::new(ReaderNavigationKey::Space, false, false, false, ReaderNavigationAction::NextPage),
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowLeft, true, false, false, ReaderNavigationAction::PreviousSection),
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowRight, true, false, false, ReaderNavigationAction::NextSection),
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowLeft, false, true, false, ReaderNavigationAction::HistoryBack),
    ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowRight, false, true, false, ReaderNavigationAction::HistoryForward),
];

pub fn resolve_reader_navigation_shortcut(key: ReaderNavigationKey, primary_modifier: bool, alt: bool, shift: bool) -> Option<ReaderNavigationAction> {
    READER_NAVIGATION_SHORTCUTS.iter().find(|shortcut| shortcut.key == key && shortcut.primary_modifier == primary_modifier && shortcut.alt == alt && shortcut.shift == shift).map(|shortcut| shortcut.action)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct ReaderNavigationContract {
    pub shortcuts: Vec<ReaderNavigationShortcut>,
}

impl Default for ReaderNavigationContract {
    fn default() -> Self {
        Self { shortcuts: READER_NAVIGATION_SHORTCUTS.to_vec() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct FrontendContract {
    pub navigation: Vec<LibraryFeatureDefinition>,
    pub app_navigation: Vec<UniversalFeatureDefinition>,
    pub browser_session: BrowserSessionContract,
    pub browsing_layout: BrowsingLayoutContract,
    pub library_actions: LibraryActionsContract,
    pub store: StoreContract,
    pub browsing_states: BrowsingStatesContract,
    pub book_actions: BookActionsContract,
    pub account: AccountContract,
    pub settings: SettingsContract,
    pub browsing_content: BrowsingContentContract,
    pub browsing_preferences: BrowsingPreferencesContract,
    pub reader_preferences: ReaderPreferencesContract,
    pub reader_settings: ReaderSettingsPresentationContract,
    pub reader_navigation: ReaderNavigationContract,
    pub web_preference_profiles: WebPreferenceProfilesContract,
}

impl Default for FrontendContract {
    fn default() -> Self {
        let navigation = LIBRARY_FEATURES.into_iter().map(|id| LibraryFeatureDefinition { id, label: id.label().to_string(), icon: id.icon() }).collect();
        let app_navigation = UNIVERSAL_FEATURES.into_iter().map(|id| UniversalFeatureDefinition { id, label: id.label().to_string(), icon: id.icon() }).collect();
        Self {
            navigation,
            app_navigation,
            browser_session: BrowserSessionContract::default(),
            browsing_layout: BrowsingLayoutContract::default(),
            library_actions: LibraryActionsContract::default(),
            store: StoreContract::default(),
            browsing_states: BrowsingStatesContract::default(),
            book_actions: BookActionsContract::default(),
            account: AccountContract::default(),
            settings: SettingsContract::default(),
            browsing_content: BrowsingContentContract::default(),
            browsing_preferences: BrowsingPreferencesContract::default(),
            reader_preferences: ReaderPreferencesContract::default(),
            reader_settings: ReaderSettingsPresentationContract::default(),
            reader_navigation: ReaderNavigationContract::default(),
            web_preference_profiles: WebPreferenceProfilesContract::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_contract_has_each_shared_feature_once() {
        let contract = FrontendContract::default();
        assert_eq!(contract.navigation.iter().map(|feature| feature.id).collect::<Vec<_>>(), LIBRARY_FEATURES.to_vec());
        assert!(contract.navigation.iter().all(|feature| feature.label == feature.id.label()));
        assert_eq!(contract.navigation.len(), LIBRARY_FEATURES.len(), "every library destination must appear in navigation exactly once");
    }

    #[test]
    fn app_navigation_contract_has_each_universal_feature_once() {
        let contract = FrontendContract::default();
        assert_eq!(contract.app_navigation.iter().map(|feature| feature.id).collect::<Vec<_>>(), UNIVERSAL_FEATURES.to_vec());
        assert!(contract.app_navigation.iter().all(|feature| feature.label == feature.id.label()));
        assert!(contract.app_navigation.iter().all(|feature| feature.icon == feature.id.icon()));
        assert_eq!(contract.app_navigation.len(), UNIVERSAL_FEATURES.len(), "every app destination must appear in navigation exactly once");
    }

    #[test]
    fn navigation_contract_owns_every_destination_icon() {
        let contract = FrontendContract::default();
        assert!(contract.navigation.iter().all(|feature| feature.icon == feature.id.icon()));
        assert!(contract.app_navigation.iter().all(|feature| feature.icon == feature.id.icon()));
    }

    /// The rail draws the two lists as two groups. If a destination could sit
    /// in both, the divider between them would stop meaning anything — the
    /// separate enums are what makes that unrepresentable, and this guards the
    /// counts that go with them.
    #[test]
    fn the_two_navigation_groups_cover_every_destination() {
        assert_eq!(LIBRARY_FEATURES.len() + UNIVERSAL_FEATURES.len(), 8, "the rail draws eight destinations");
    }

    #[test]
    fn browser_session_semantics_match_the_native_browser() {
        let contract = FrontendContract::default().browser_session;
        assert_eq!(contract.default_library_route, LibraryFeature::Folders);
        assert!(contract.restore_places_per_library);
        assert!(!contract.persist_universal_routes);
    }

    /// The card's declared height has to equal what its children actually
    /// occupy, or the grid reserves space nothing fills.
    #[test]
    fn the_book_card_height_is_the_sum_of_its_children() {
        const COVER: u16 = LARGE_BOOK_COVER_HEIGHT_PX;
        const TEXT_GAP: u16 = BOOK_CARD_COVER_CONTENT_GAP_PX as u16;
        const TITLE: u16 = BOOK_CARD_TITLE_LINE_HEIGHT_PX as u16 * BOOK_CARD_TITLE_LINES as u16;
        const ROW_GAP: u16 = BOOK_CARD_TEXT_GAP_PX as u16;
        const AUTHOR: u16 = BOOK_CARD_AUTHOR_LINE_HEIGHT_PX as u16;
        const PROGRESS: u16 = BOOK_CARD_PROGRESS_HEIGHT_PX as u16;
        assert_eq!(LARGE_BOOK_CARD_HEIGHT_PX, COVER + TEXT_GAP + TITLE + ROW_GAP + AUTHOR + ROW_GAP + PROGRESS);
        assert_eq!(LARGE_BOOK_GRID_ROW_HEIGHT_PX, LARGE_BOOK_CARD_HEIGHT_PX + BOOK_GRID_ROW_TRAILING_SPACE_PX as u16);
        let layout = FrontendContract::default().browsing_layout;
        assert_eq!(layout.book_card_title_lines, 2);
        assert_eq!(layout.book_card_title_weight, 600);
    }

    /// The row is virtualized on a fixed height, so the thumbnail must fit
    /// inside it and the text block must not exceed the thumbnail — otherwise
    /// content is clipped at a height nothing declares.
    #[test]
    fn the_detailed_row_holds_its_thumbnail_and_text() {
        assert_eq!(DETAILED_ROW_THUMBNAIL_HEIGHT_PX + DETAILED_ROW_VERTICAL_PADDING_PX * 2, DETAILED_ROW_HEIGHT_PX, "the thumbnail and its padding must fit the row exactly");

        const TITLE: u16 = 22;
        const AUTHOR: u16 = 20;
        const DESCRIPTION: u16 = 84;
        const PROGRESS: u16 = 16;
        const GAP: u16 = 5;
        let text = TITLE + GAP + AUTHOR + GAP + DESCRIPTION + GAP + PROGRESS;
        assert!(text <= DETAILED_ROW_THUMBNAIL_HEIGHT_PX, "the text block must not outgrow the art beside it");
    }

    #[test]
    fn the_detailed_row_thumbnail_matches_the_cover_ratio() {
        assert_eq!(
            f64::from(DETAILED_ROW_THUMBNAIL_HEIGHT_PX) / f64::from(DETAILED_ROW_THUMBNAIL_WIDTH_PX),
            f64::from(LARGE_BOOK_COVER_HEIGHT_PX) / f64::from(LARGE_BOOK_COVER_WIDTH_PX),
            "both presentations draw the same asset, so they must not crop it differently"
        );
    }

    #[test]
    fn metadata_review_modes_share_one_fixed_item_height() {
        assert_eq!(METADATA_REVIEW_ITEM_HEIGHT_PX, 376);
    }

    #[test]
    fn book_covers_are_the_declared_aspect_ratio() {
        assert_eq!(f64::from(LARGE_BOOK_COVER_HEIGHT_PX) / f64::from(LARGE_BOOK_COVER_WIDTH_PX), 1.6);
    }

    #[test]
    fn author_cards_reuse_regular_book_covers() {
        assert_eq!(AUTHOR_ROW_COVER_COUNT, 3);
        assert_eq!(AUTHOR_ROW_COVER_WIDTH_PX, LARGE_BOOK_COVER_WIDTH_PX);
        assert_eq!(AUTHOR_ROW_COVER_HEIGHT_PX, LARGE_BOOK_COVER_HEIGHT_PX);
        assert_eq!(AUTHOR_ROW_HEIGHT_PX, AUTHOR_ROW_HORIZONTAL_PADDING_PX as u16 * 2 + BOOK_CARD_TITLE_LINE_HEIGHT_PX as u16 * BOOK_CARD_TITLE_LINES as u16 + AUTHOR_ROW_CONTENT_GAP_PX as u16 + AUTHOR_ROW_COVER_HEIGHT_PX);
    }

    #[test]
    fn browsing_content_contract_has_safe_shared_defaults() {
        let contract = FrontendContract::default().browsing_content;
        assert_eq!(contract.minimum_search_characters, 1);
        assert_eq!(contract.search_debounce_ms, 220);
        assert_eq!(contract.search_result_limit, 100);
        assert!(contract.import_extensions.iter().any(|extension| extension == "pdf"));
        assert!(contract.semantic_link_relations.iter().any(|relation| relation == "related to"));
    }

    #[test]
    fn hierarchy_geometry_distinguishes_pointer_and_touch_density() {
        let layout = FrontendContract::default().browsing_layout;
        assert_eq!(layout.hierarchy_cell_height_px, 40);
        assert_eq!(layout.hierarchy_touch_cell_min_height_px, 56);
        assert_eq!(layout.hierarchy_cell_min_width_px, 240);
        assert_eq!(layout.hierarchy_cell_gap_px, 4);
        assert_eq!(layout.hierarchy_icon_size_px, 22);
        assert_eq!(layout.hierarchy_label_size_px, 18);
    }

    #[test]
    fn shared_surfaces_use_square_corners() {
        assert_eq!(FrontendContract::default().browsing_layout.surface_corner_radius_px, 0);
    }

    #[test]
    fn topbar_actions_use_semibold_labels() {
        assert_eq!(FrontendContract::default().browsing_layout.topbar_action_label_weight, 600);
    }

    #[test]
    fn navigation_labels_share_a_readable_responsive_size() {
        assert_eq!(FrontendContract::default().browsing_layout.navigation_label_size_px, 12);
    }

    #[test]
    fn navigation_labels_share_semibold_weight() {
        assert_eq!(FrontendContract::default().browsing_layout.navigation_label_weight, 600);
    }

    #[test]
    fn navigation_icons_share_one_responsive_size() {
        assert_eq!(FrontendContract::default().browsing_layout.navigation_icon_size_px, 24);
    }

    #[test]
    fn navigation_icons_and_labels_share_one_gap() {
        assert_eq!(FrontendContract::default().browsing_layout.navigation_icon_label_gap_px, 5);
    }

    #[test]
    fn navigation_heights_are_minimums_for_pointer_and_touch_layouts() {
        let layout = FrontendContract::default().browsing_layout;
        assert_eq!(layout.navigation_item_min_height_px, 64);
        assert_eq!(layout.navigation_touch_item_min_height_px, 56);
    }

    #[test]
    fn book_action_labels_are_shared_by_frontends() {
        let actions = FrontendContract::default().book_actions;
        assert_eq!(actions.menu_label, "Book actions");
        assert_eq!(actions.download_label, "Download");
        assert_eq!(actions.download_in_progress_label, "Download in progress");
        assert_eq!(actions.remove_local_copy_label, "Remove local copy");
    }

    #[test]
    fn account_and_settings_contracts_share_shell_semantics() {
        let contract = FrontendContract::default();
        assert_eq!(contract.account.navigation_label, "Account");
        assert!(contract.account.capabilities.public_registration);
        assert!(contract.account.capabilities.password_recovery);
        assert_eq!(contract.account.validation.password_min_characters, 12);
        assert_eq!(contract.account.sync_states.len(), 5);
        assert_eq!(contract.settings.navigation_label, "Settings");
        assert_eq!(contract.settings.network_transports.len(), 7);
    }

    #[test]
    fn large_browsing_geometry_and_library_actions_are_shared() {
        let contract = FrontendContract::default();
        assert_eq!(contract.browsing_layout.book_cover_width_px, LARGE_BOOK_COVER_WIDTH_PX);
        assert_eq!(contract.browsing_layout.book_cover_height_px, LARGE_BOOK_COVER_HEIGHT_PX);
        assert_eq!(contract.browsing_layout.sidebar_width_px, LARGE_SIDEBAR_WIDTH_PX);
        assert!(!contract.library_actions.show_bulk_download_in_topbar);
        assert_eq!(contract.browsing_states.create_library_label, "Create library");
        assert_eq!(contract.store.max_width_px, STORE_MAX_WIDTH_PX);
        assert_eq!(contract.store.publication_min_width_px, STORE_PUBLICATION_MIN_WIDTH_PX);
        assert_eq!(contract.store.catalogs_label, "Catalogs");
    }

    #[test]
    fn reader_navigation_matches_native_baseline() {
        let contract = FrontendContract::default();
        assert_eq!(contract.reader_navigation.shortcuts, READER_NAVIGATION_SHORTCUTS);
        assert_eq!(resolve_reader_navigation_shortcut(ReaderNavigationKey::ArrowLeft, true, false, false), Some(ReaderNavigationAction::PreviousSection));
        assert_eq!(resolve_reader_navigation_shortcut(ReaderNavigationKey::ArrowRight, false, true, false), Some(ReaderNavigationAction::HistoryForward));
        assert_eq!(resolve_reader_navigation_shortcut(ReaderNavigationKey::ArrowRight, false, false, true), None);
    }

    #[test]
    fn reader_settings_are_a_shared_contextual_overlay() {
        let settings = FrontendContract::default().reader_settings;
        assert_eq!(settings.presentation, ReaderSettingsPresentation::ContextualOverlay);
        assert_eq!(settings.anchor, ReaderSettingsAnchor::ReaderToolbarEnd);
        assert_eq!(settings.trigger_label, "Aa");
        assert_eq!(settings.title, "Appearance");
        assert!(settings.keeps_document_visible);
        assert_eq!(settings.max_width_px, READER_SETTINGS_OVERLAY_MAX_WIDTH_PX);
    }
}
