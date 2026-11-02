//! Wording for failures crossing the backend/UI boundary. Errors arrive
//! classified by where they arose; their text is only logged.

use ui_text::text;

use operation_failure::{Classified, Failure, WifiFailure};

/// A known recovery step for `error`, or the caller's action-specific
/// fallback. Never shows the error's own text.
pub fn user_facing_error(error: &(impl Classified + ?Sized), fallback: &'static str) -> &'static str {
    log::warn!("{fallback} Diagnostic: {error}");
    error.failure().and_then(recovery).unwrap_or(fallback)
}

fn recovery(failure: Failure) -> Option<&'static str> {
    Some(match failure {
        Failure::StorageFull => text!(common::not_enough_storage_free_up_space_on_the_system_drive_library_drive),
        Failure::CloudStorageFull => text!(common::cloud_storage_is_full_free_up_space_in_your_account_then_retry),
        Failure::ReadOnlyStorage => text!(common::this_location_is_read_only_allow_changes_or_choose_a_writable_library),
        Failure::PermissionDenied => text!(common::access_denied_check_that_you_have_permission_to_use_the_folder),
        Failure::StorageBusy => text!(common::another_tab_is_using_browser_storage_close_other_app_tabs_then_reload),
        Failure::WebViewMissing => text!(common::the_system_browser_component_is_missing_install_webkitgtk_4_1_then_reopen),
        Failure::InvalidAddress => text!(common::enter_a_complete_web_address_starting_with_https_or_http),
        Failure::NotACatalog => text!(common::no_supported_catalog_at_this_address_check_the_feed_address_and_retry),
        Failure::CatalogCredentialsRejected => text!(common::catalog_sign_in_failed_check_your_username_and_password_then_retry),
        Failure::Unauthorized | Failure::SignInRequired => text!(common::sign_in_again_then_retry_the_action),
        Failure::Forbidden => text!(common::access_denied_check_your_account_s_permissions_then_retry),
        Failure::NotFound => text!(common::item_unavailable_refresh_or_choose_another_item),
        Failure::RateLimited => text!(common::too_many_requests_please_wait_a_few_minutes_then_retry),
        Failure::ServiceUnavailable => text!(common::service_unavailable_please_try_again_later),
        Failure::ClientUpgradeRequired => text!(common::client_upgrade_required),
        Failure::PasswordTooLong => text!(common::password_too_long_use_a_shorter_one),
        Failure::TimedOut => text!(common::request_timed_out_check_your_connection_and_retry),
        Failure::Offline => text!(common::couldn_t_connect_check_your_internet_connection_and_retry),
        Failure::LibraryFolderUnavailable => text!(common::library_folder_unavailable_reconnect_its_drive_or_choose_another_library),
        Failure::AccountChanged => text!(common::your_account_changed_during_this_action_retry),
        Failure::DrmProtected => text!(common::drm_protected_books_aren_t_supported),
        Failure::UnreadableBook => text!(common::couldn_t_read_the_book_file_download_it_again_or_try_another),
        Failure::BookFileUnavailable => text!(common::couldn_t_access_this_book_s_file_check_the_library_folder_and),
        Failure::InvalidBookContainer => text!(common::couldn_t_open_the_book_s_archive_or_file_headers_it_may),
        Failure::InvalidBookContent => text!(common::couldn_t_read_the_book_s_chapters_or_reading_order_try_another),
        Failure::BookDocumentFailed => text!(common::a_chapter_couldn_t_be_prepared_for_display_try_another_chapter_or),
        Failure::BookRendererFailed => text!(common::the_reader_couldn_t_display_the_prepared_book_reopen_the_app_if),
        Failure::ReadingPositionUnavailable => text!(common::couldn_t_load_your_saved_reading_position_reopen_the_book_its_file),
        Failure::Wifi(failure) => wifi_message(failure),
        Failure::Rejected => return None,
    })
}

/// Wi-Fi failures need recovery steps that work while the device is offline.
pub fn wifi_error_message(error: &(impl Classified + ?Sized)) -> &'static str {
    log::warn!("Wi-Fi request failed: {error}");
    match error.failure() {
        Some(Failure::Wifi(failure)) => wifi_message(failure),
        _ => text!(common::couldn_t_update_wi_fi_retry_or_restart_the_app),
    }
}

fn wifi_message(failure: WifiFailure) -> &'static str {
    match failure {
        WifiFailure::InvalidPassword => text!(common::enter_the_network_password_using_8_to_63_standard_keyboard_characters),
        WifiFailure::AuthenticationFailed => text!(common::wi_fi_password_rejected_check_the_network_password_and_retry),
        WifiFailure::UnsupportedSecurity => text!(common::this_network_s_security_type_isn_t_supported_try_another_network),
        WifiFailure::NetworkGone => text!(common::wi_fi_network_unavailable_tap_refresh_and_choose_another_network),
        WifiFailure::InvalidNetworkName => text!(common::enter_a_network_name_if_you_ve_already_entered_one_try_a),
        WifiFailure::ScanTimedOut => text!(common::wi_fi_scan_timed_out_tap_refresh_to_retry),
        WifiFailure::NoAddress => text!(common::couldn_t_get_a_network_address_reconnect_or_check_your_router),
        WifiFailure::ConnectTimedOut => text!(common::wi_fi_connection_timed_out_check_the_password_move_closer_to_the),
        WifiFailure::Sleeping => text!(common::wi_fi_is_unavailable_during_sleep_wake_the_device_and_retry),
        WifiFailure::PowerControlUnavailable => text!(common::couldn_t_switch_wi_fi_on_or_off_use_the_device_s),
    }
}

/// The account operations whose failures the forms explain, shared by the
/// desktop, Android, and browser account screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountAction {
    Login,
    Register,
    Verify,
    Resend,
    RequestReset,
    Reset,
    DeleteAccount,
}

pub fn authentication_error(error: &(impl Classified + ?Sized), action: AccountAction) -> &'static str {
    log::warn!("Account action {action:?} failed: {error}");
    let failure = error.failure();
    // The server refused these credentials or this request as made.
    let refused = matches!(failure, Some(Failure::Unauthorized | Failure::Rejected));
    let explained = match action {
        AccountAction::DeleteAccount => match failure {
            Some(Failure::Unauthorized) => Some(text!(common::the_password_is_incorrect_check_your_current_password_and_retry)),
            Some(Failure::NotFound) => Some(text!(common::account_deletion_is_not_available_on_the_server_try_again_later)),
            Some(Failure::AccountChanged) => Some(text!(common::your_account_changed_reopen_account_deletion_and_retry)),
            _ => None,
        },
        AccountAction::Login if refused => Some(text!(common::couldn_t_sign_in_check_your_email_and_password_or_reset_a)),
        AccountAction::Verify if refused => Some(text!(common::couldn_t_verify_your_email_enter_the_latest_emailed_pin_or_request)),
        AccountAction::Reset if refused => Some(text!(common::couldn_t_reset_your_password_request_a_new_reset_code_and_use)),
        AccountAction::Register if refused => Some(text!(common::couldn_t_create_the_account_check_your_email_and_password_if_you)),
        _ => None,
    };
    let fallback = match action {
        AccountAction::DeleteAccount => text!(common::couldn_t_confirm_account_deletion_check_your_connection_and_account_status_then),
        AccountAction::Login => text!(common::couldn_t_sign_in_try_again),
        AccountAction::Verify => text!(common::couldn_t_verify_your_email_try_again),
        AccountAction::Reset => text!(common::couldn_t_reset_your_password_try_again),
        AccountAction::Register => text!(common::couldn_t_create_the_account_try_again),
        AccountAction::Resend => text!(common::couldn_t_send_a_verification_pin_check_your_email_address_and_retry),
        AccountAction::RequestReset => text!(common::couldn_t_send_a_password_reset_email_check_your_email_address_and),
    };
    explained.or_else(|| failure.and_then(recovery)).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::{AccountAction, authentication_error, user_facing_error, wifi_error_message};
    use operation_failure::{Classified, Failure, WifiFailure};
    use std::fmt;

    struct Failed(Option<Failure>);
    impl fmt::Display for Failed {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("private diagnostic /home/reader")
        }
    }
    impl Classified for Failed {
        fn failure(&self) -> Option<Failure> {
            self.0
        }
    }

    #[test]
    fn deletion_errors_explain_password_and_endpoint_failures() {
        assert!(authentication_error(&Failed(Some(Failure::Unauthorized)), AccountAction::DeleteAccount).contains("password is incorrect"));
        assert!(authentication_error(&Failed(Some(Failure::NotFound)), AccountAction::DeleteAccount).contains("not available on the server"));
        assert!(authentication_error(&Failed(Some(Failure::AccountChanged)), AccountAction::DeleteAccount).contains("Reopen account deletion"));
    }

    #[test]
    fn unclassified_errors_show_the_fallback_never_their_text() {
        let fallback = "Could not save this change. Please try again.";
        assert_eq!(user_facing_error(&Failed(None), fallback), fallback);
        assert_eq!(user_facing_error("No space left on device (os error 28)", fallback), fallback);
        assert_eq!(user_facing_error(&Failed(Some(Failure::Rejected)), fallback), fallback);
        assert!(!authentication_error(&Failed(None), AccountAction::Login).contains("private"));
    }

    #[test]
    fn book_opening_messages_distinguish_content_display_and_saved_state() {
        let fallback = "Could not open this book.";
        for (failure, expected) in [
            (Failure::BookFileUnavailable, "access this book's file"),
            (Failure::InvalidBookContainer, "damaged or unsupported"),
            (Failure::InvalidBookContent, "book’s chapters"),
            (Failure::BookDocumentFailed, "open this chapter"),
            (Failure::BookRendererFailed, "display this book"),
            (Failure::ReadingPositionUnavailable, "saved reading position"),
        ] {
            let message = user_facing_error(&Failed(Some(failure)), fallback);
            assert!(message.contains(expected), "{failure:?}: {message}");
            assert!(!message.contains("private"));
        }
    }

    #[test]
    fn authentication_recovery_depends_on_the_action() {
        let refused = Failed(Some(Failure::Unauthorized));
        assert!(authentication_error(&refused, AccountAction::Login).contains("Check your email and password"));
        assert!(authentication_error(&refused, AccountAction::Verify).contains("request a new PIN"));
        assert!(authentication_error(&refused, AccountAction::Reset).contains("new reset code"));
        assert!(authentication_error(&Failed(Some(Failure::RateLimited)), AccountAction::Login).contains("wait a few minutes"));
        assert!(authentication_error(&Failed(Some(Failure::ServiceUnavailable)), AccountAction::Register).contains("try again later"));
    }

    #[test]
    fn wifi_recovery_does_not_require_an_existing_internet_connection() {
        assert!(wifi_error_message(&Failed(Some(Failure::Wifi(WifiFailure::AuthenticationFailed)))).contains("Check the network password"));
        assert!(wifi_error_message(&Failed(Some(Failure::Wifi(WifiFailure::ScanTimedOut)))).contains("Tap Refresh"));
        let message = wifi_error_message(&Failed(None));
        assert!(!message.contains("private"));
        assert!(message.contains("restart the app"));
    }
}
