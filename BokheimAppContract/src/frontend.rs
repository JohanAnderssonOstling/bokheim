use crate::account::LibrarySyncState;
use crate::preferences::{BrowsingPreferencesContract, ReaderPreferencesContract, WebPreferenceProfilesContract};
use crate::transfer::NetworkTransport;
use bokheim_account_contract::{EMAIL_MAX_BYTES, ONE_TIME_TOKEN_BYTES, PASSWORD_MAX_BYTES, PASSWORD_MIN_CHARACTERS};
use serde::{Deserialize, Serialize};
use specta::Type;

pub const BOOK_IMPORT_EXTENSIONS: [&str; 11] = ["epub", "pdf", "mobi", "azw", "azw3", "fb2", "cbz", "html", "htm", "xhtml", "webpub"];
pub const SEMANTIC_LINK_RELATIONS: [&str; 8] = ["related to", "continues", "continued by", "inspired by", "adaptation of", "contrasts with", "commentary on", "translation of"];
pub const BREADCRUMB_ITEM_MAX_CHARACTERS: u8 = 24;

pub const LARGE_VIEWPORT_BREAKPOINT_PX: u16 = 720;
pub const LARGE_SIDEBAR_WIDTH_PX: u16 = 96;
pub const LARGE_TOPBAR_HEIGHT_PX: u16 = 64;
pub const LARGE_TOPBAR_ACTION_HEIGHT_PX: u16 = 40;
pub const LARGE_TOPBAR_ACTION_GAP_PX: u16 = 10;
pub const LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX: u16 = 11;
pub const LARGE_CONTENT_PADDING_PX: u16 = 18;
pub const LARGE_BOOK_COVER_WIDTH_PX: u16 = 204;
pub const LARGE_BOOK_COVER_HEIGHT_PX: u16 = 306;
pub const LARGE_BOOK_CARD_HEIGHT_PX: u16 = 432;
pub const LARGE_BOOK_GRID_COLUMN_GAP_PX: u16 = 28;
pub const LARGE_BOOK_GRID_ROW_HEIGHT_PX: u16 = 438;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BrowsingLayoutContract {
    pub large_viewport_breakpoint_px: u16,
    pub sidebar_width_px: u16,
    pub topbar_height_px: u16,
    pub topbar_action_height_px: u16,
    pub topbar_action_gap_px: u16,
    pub topbar_action_horizontal_padding_px: u16,
    pub content_padding_px: u16,
    pub book_cover_width_px: u16,
    pub book_cover_height_px: u16,
    pub book_card_height_px: u16,
    pub book_grid_column_gap_px: u16,
    pub book_grid_row_height_px: u16,
}

impl Default for BrowsingLayoutContract {
    fn default() -> Self {
        Self {
            large_viewport_breakpoint_px: LARGE_VIEWPORT_BREAKPOINT_PX,
            sidebar_width_px: LARGE_SIDEBAR_WIDTH_PX,
            topbar_height_px: LARGE_TOPBAR_HEIGHT_PX,
            topbar_action_height_px: LARGE_TOPBAR_ACTION_HEIGHT_PX,
            topbar_action_gap_px: LARGE_TOPBAR_ACTION_GAP_PX,
            topbar_action_horizontal_padding_px: LARGE_TOPBAR_ACTION_HORIZONTAL_PADDING_PX,
            content_padding_px: LARGE_CONTENT_PADDING_PX,
            book_cover_width_px: LARGE_BOOK_COVER_WIDTH_PX,
            book_cover_height_px: LARGE_BOOK_COVER_HEIGHT_PX,
            book_card_height_px: LARGE_BOOK_CARD_HEIGHT_PX,
            book_grid_column_gap_px: LARGE_BOOK_GRID_COLUMN_GAP_PX,
            book_grid_row_height_px: LARGE_BOOK_GRID_ROW_HEIGHT_PX,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
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
    pub semantic_link_relations: Vec<String>,
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
    pub one_time_token_bytes: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct LibrarySyncStateDefinition {
    pub state: LibrarySyncState,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AccountContract {
    pub navigation_label: String,
    pub title: String,
    pub libraries_section_label: String,
    pub transfers_section_label: String,
    pub no_local_libraries_label: String,
    pub no_active_transfers_label: String,
    pub refresh_label: String,
    pub sign_out_label: String,
    pub capabilities: AccountCapabilities,
    pub validation: AccountValidationContract,
    pub sync_states: Vec<LibrarySyncStateDefinition>,
}

impl Default for AccountContract {
    fn default() -> Self {
        let sync_states = [
            (LibrarySyncState::LocalOnly, "Local", "Stored only on this device"),
            (LibrarySyncState::Synced, "Synced", "Synchronized with the account"),
            (LibrarySyncState::SignInRequired, "Sign in required", "Sign in to resume synchronization"),
            (LibrarySyncState::DifferentAccount, "Different account", "Linked to another account"),
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
            no_local_libraries_label: "No local libraries".to_owned(),
            no_active_transfers_label: "No active batch jobs".to_owned(),
            refresh_label: "Refresh".to_owned(),
            sign_out_label: "Sign out".to_owned(),
            capabilities: AccountCapabilities { public_registration: true, email_verification: true, resend_verification: true, password_recovery: true },
            validation: AccountValidationContract {
                password_min_characters: PASSWORD_MIN_CHARACTERS as u16,
                password_max_bytes: PASSWORD_MAX_BYTES as u16,
                email_max_bytes: EMAIL_MAX_BYTES as u16,
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
    pub application_theme_label: String,
    pub interface_font_label: String,
    pub heading_font_label: String,
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
            application_theme_label: "Application theme".to_owned(),
            interface_font_label: "Interface font".to_owned(),
            heading_font_label: "Heading font".to_owned(),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppFeature {
    Home,
    Library,
    Authors,
    Search,
    Store,
}

impl AppFeature {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Library => "Library",
            Self::Authors => "Authors",
            Self::Search => "Search",
            Self::Store => "Store",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct FeatureDefinition {
    pub id: AppFeature,
    pub label: String,
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
    pub navigation: Vec<FeatureDefinition>,
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
        let navigation = [AppFeature::Home, AppFeature::Library, AppFeature::Authors, AppFeature::Search, AppFeature::Store].into_iter().map(|id| FeatureDefinition { id, label: id.label().to_string() }).collect();
        Self {
            navigation,
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
        assert_eq!(contract.navigation.iter().map(|feature| feature.id).collect::<Vec<_>>(), vec![AppFeature::Home, AppFeature::Library, AppFeature::Authors, AppFeature::Search, AppFeature::Store]);
        assert!(contract.navigation.iter().all(|feature| feature.label == feature.id.label()));
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
