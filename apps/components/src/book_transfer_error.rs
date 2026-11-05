//! Actionable messages for book downloads and imports, worded from how the
//! transfer failed. Diagnostics stay in the logs where they arose.

use ui_text::text;

use operation_failure::Failure;

pub fn book_transfer_error_message(failure: Option<Failure>) -> &'static str {
    match failure {
        Some(Failure::DrmProtected) => text!(common::drm_protected_downloads_aren_t_supported),
        Some(Failure::StorageFull) => text!(common::not_enough_storage_to_save_this_book_free_up_space_on_the),
        Some(Failure::CloudStorageFull) => text!(common::cloud_storage_is_full_free_up_account_space_and_retry_your_local),
        Some(Failure::ReadOnlyStorage) => text!(common::your_library_is_read_only_allow_changes_or_choose_a_writable_library),
        Some(Failure::PermissionDenied) => text!(common::couldn_t_save_to_your_library_check_its_folder_permissions_or_choose),
        Some(Failure::NotFound) => text!(common::the_download_link_is_no_longer_available_try_another_edition_or_the),
        Some(Failure::Unauthorized | Failure::SignInRequired) => text!(common::sign_in_to_the_catalog_then_retry_the_download),
        Some(Failure::Forbidden) => text!(common::the_source_blocked_this_download_try_its_website_or_another_edition),
        Some(Failure::RateLimited | Failure::ServiceUnavailable) => text!(common::download_service_unavailable_try_again_later),
        Some(Failure::TimedOut) => text!(common::download_timed_out_check_your_internet_connection_and_retry),
        Some(Failure::Offline) => text!(common::download_couldn_t_finish_check_your_internet_connection_and_retry),
        Some(Failure::UnreadableBook) => text!(common::couldn_t_open_the_book_file_download_it_again_or_try_another),
        Some(Failure::LibraryFolderUnavailable) => text!(common::library_folder_unavailable_reconnect_its_drive_or_choose_another_library),
        Some(Failure::ClientUpgradeRequired) => text!(common::this_client_and_server_are_incompatible_reload_the_web_app_or_check),
        _ => text!(common::couldn_t_download_or_save_the_book_retry_or_try_another_edition),
    }
}
