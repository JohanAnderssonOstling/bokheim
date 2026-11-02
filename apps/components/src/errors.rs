//! Wording for failures crossing the backend/UI boundary. Errors arrive
//! classified by where they arose; their text is only logged.

use operation_failure::{Classified, Failure, WifiFailure};

/// A known recovery step for `error`, or the caller's action-specific
/// fallback. Never shows the error's own text.
pub fn user_facing_error(error: &(impl Classified + ?Sized), fallback: &'static str) -> &'static str {
    log::warn!("{fallback} Diagnostic: {error}");
    error.failure().and_then(recovery).unwrap_or(fallback)
}

fn recovery(failure: Failure) -> Option<&'static str> {
    Some(match failure {
        Failure::StorageFull => "Not enough storage. Free up space on the device or drive holding your library, then retry.",
        Failure::CloudStorageFull => "Cloud storage is full. Free up space in your account, then retry.",
        Failure::ReadOnlyStorage => "This location is read-only. Allow changes or choose a writable library folder.",
        Failure::PermissionDenied => "Access denied. Check that you have permission to use the folder.",
        Failure::StorageBusy => "Another tab is using browser storage. Close other app tabs, then reload.",
        Failure::WebViewMissing => "The system browser component is missing. Install WebKitGTK 4.1, then reopen the app.",
        Failure::InvalidAddress => "Enter a complete web address starting with https:// or http://.",
        Failure::NotACatalog => "No supported catalog at this address. Check the feed address and retry.",
        Failure::CatalogCredentialsRejected => "Catalog sign-in failed. Check your username and password, then retry.",
        Failure::Unauthorized | Failure::SignInRequired => "Sign in again, then retry the action.",
        Failure::Forbidden => "Access denied. Check your account's permissions, then retry.",
        Failure::NotFound => "Item unavailable. Refresh or choose another item.",
        Failure::RateLimited => "Too many requests. Please wait a few minutes, then retry.",
        Failure::ServiceUnavailable => "Service unavailable. Please try again later.",
        Failure::ClientUpgradeRequired => "This client and server are incompatible. Reload the web app or check for app updates. If it persists, report the problem.",
        Failure::PasswordTooLong => "Password too long. Use a shorter one.",
        Failure::TimedOut => "Request timed out. Check your connection and retry.",
        Failure::Offline => "Couldn't connect. Check your internet connection and retry.",
        Failure::LibraryFolderUnavailable => "Library folder unavailable. Reconnect its drive or choose another library.",
        Failure::AccountChanged => "Your account changed during this action. Retry.",
        Failure::DrmProtected => "DRM-protected books aren't supported.",
        Failure::UnreadableBook => "Couldn't read the book file. Download it again or try another edition.",
        Failure::BookFileUnavailable => "Couldn't access this book's file. Check the library folder and drive, or download it again.",
        Failure::InvalidBookContainer => "Couldn't open the book's archive or file headers. It may be incomplete or unsupported. Download it again or try another edition.",
        Failure::InvalidBookContent => "Couldn't read the book's chapters or reading order. Try another edition or repair the file.",
        Failure::BookDocumentFailed => "A chapter couldn't be prepared for display. Try another chapter or edition.",
        Failure::BookRendererFailed => "The reader couldn't display the prepared book. Reopen the app. If it persists, report the problem.",
        Failure::ReadingPositionUnavailable => "Couldn't load your saved reading position. Reopen the book; its file may still be intact.",
        Failure::Wifi(failure) => wifi_message(failure),
        Failure::Rejected => return None,
    })
}

/// Wi-Fi failures need recovery steps that work while the device is offline.
pub fn wifi_error_message(error: &(impl Classified + ?Sized)) -> &'static str {
    log::warn!("Wi-Fi request failed: {error}");
    match error.failure() {
        Some(Failure::Wifi(failure)) => wifi_message(failure),
        _ => "Couldn't update Wi-Fi. Retry or restart the app.",
    }
}

fn wifi_message(failure: WifiFailure) -> &'static str {
    match failure {
        WifiFailure::InvalidPassword => "Enter the network password using 8 to 63 standard keyboard characters.",
        WifiFailure::AuthenticationFailed => "Wi-Fi password rejected. Check the network password and retry.",
        WifiFailure::UnsupportedSecurity => "This network's security type isn't supported. Try another network.",
        WifiFailure::NetworkGone => "Wi-Fi network unavailable. Tap Refresh and choose another network.",
        WifiFailure::InvalidNetworkName => "Enter a network name. If you've already entered one, try a shorter name.",
        WifiFailure::ScanTimedOut => "Wi-Fi scan timed out. Tap Refresh to retry.",
        WifiFailure::NoAddress => "Couldn't get a network address. Reconnect or check your router.",
        WifiFailure::ConnectTimedOut => "Wi-Fi connection timed out. Check the password, move closer to the router, then retry.",
        WifiFailure::Sleeping => "Wi-Fi is unavailable during sleep. Wake the device and retry.",
        WifiFailure::PowerControlUnavailable => "Couldn't switch Wi-Fi on or off. Use the device's Wi-Fi controls.",
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
            Some(Failure::Unauthorized) => Some("The password is incorrect. Check your current password and retry."),
            Some(Failure::NotFound) => Some("Account deletion is not available on the server. Try again later."),
            Some(Failure::AccountChanged) => Some("Your account changed. Reopen account deletion and retry."),
            _ => None,
        },
        AccountAction::Login if refused => Some("Couldn't sign in. Check your email and password, or reset a forgotten password."),
        AccountAction::Verify if refused => Some("Couldn't verify your email. Enter the latest emailed PIN or request a new PIN. If it persists, restart account creation."),
        AccountAction::Reset if refused => Some("Couldn't reset your password. Request a new reset code and use a password that meets the length requirement."),
        AccountAction::Register if refused => Some("Couldn't create the account. Check your email and password. If you have an account, sign in or reset its password."),
        _ => None,
    };
    let fallback = match action {
        AccountAction::DeleteAccount => "Couldn't confirm account deletion. Check your connection and account status, then retry.",
        AccountAction::Login => "Couldn't sign in. Try again.",
        AccountAction::Verify => "Couldn't verify your email. Try again.",
        AccountAction::Reset => "Couldn't reset your password. Try again.",
        AccountAction::Register => "Couldn't create the account. Try again.",
        AccountAction::Resend => "Couldn't send a verification PIN. Check your email address and retry.",
        AccountAction::RequestReset => "Couldn't send a password reset email. Check your email address and retry.",
    };
    explained.or_else(|| failure.and_then(recovery)).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::{authentication_error, user_facing_error, wifi_error_message, AccountAction};
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
            (Failure::InvalidBookContainer, "archive or file headers"),
            (Failure::InvalidBookContent, "chapters or reading order"),
            (Failure::BookDocumentFailed, "chapter couldn't be prepared"),
            (Failure::BookRendererFailed, "reader couldn't display"),
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
