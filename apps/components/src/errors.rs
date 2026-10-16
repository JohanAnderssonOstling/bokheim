//! Presentation for diagnostics crossing the backend/UI boundary.
//! Keep diagnostics in logs; show only wording chosen by the application.

/// Returns a known recovery step, or the caller's action-specific fallback.
/// Never use backend text as the fallback.
pub fn user_facing_error(diagnostic: &str, fallback: &'static str) -> &'static str {
    log::warn!("{fallback} Diagnostic: {diagnostic}");
    friendly_error(diagnostic, fallback)
}

fn friendly_error(diagnostic: &str, fallback: &'static str) -> &'static str {
    let error = diagnostic.to_ascii_lowercase();
    let has = |terms: &[&str]| terms.iter().any(|term| error.contains(term));

    if has(&["no space left", "os error 28", "os error 112", "disk full", "database or disk is full", "disk quota exceeded", "not enough storage", "not enough space", "quotaexceedederror"]) {
        "There isn't enough free space. Free up space on the device or drive that holds your library, then try again."
    } else if has(&["read-only file system", "readonly filesystem", "readonly database", "read-only database", "os error 30"]) {
        "Bokheim can't save changes here. Allow changes to the library folder or choose a writable library location, then try again."
    } else if has(&["permission denied", "access is denied", "os error 13"]) {
        "Bokheim can't access the files it needs. Check that you have permission to use the folder, then try again."
    } else if has(&["creating sync access handle", "bokheim_storage_busy"]) {
        "Another Bokheim tab is using browser storage. Close your other Bokheim tabs, then reload this page."
    } else if has(&["needs webkitgtk 4.1", "libwebkit2gtk-4.1"]) {
        "The system browser component is missing. Install WebKitGTK 4.1, then reopen Bokheim and try again."
    } else if has(&["not a web address", "not a feed address", "a store needs a web address", "relative url without a base"]) {
        "Enter a complete web address starting with https:// or http://."
    } else if has(&["not an opds feed", "not an opds 2 feed", "feed is not utf-8"]) {
        "This address doesn't provide a supported book catalog. Check the catalog's feed address and try again."
    } else if has(&["feed refused this username and password"]) {
        "The catalog didn't accept those sign-in details. Check your username and password, then try again."
    } else if is_http_status(&error, 401) || has(&["not signed in", "sign in again", "sign-in required", "feed asks to sign in"]) {
        "Sign in again, then retry the action."
    } else if is_http_status(&error, 403) {
        "Access was refused. Check that you're signed in with an account that has access, then try again."
    } else if is_http_status(&error, 404) || is_http_status(&error, 410) || has(&["no longer listed", "page is no longer known"]) {
        "This item is no longer available. Refresh the page or choose another item."
    } else if is_http_status(&error, 429) {
        "Too many requests were made. Please wait a few minutes, then try again."
    } else if (500..=599).any(|code| is_http_status(&error, code)) {
        "The service is temporarily unavailable. Please try again later."
    } else if has(&["timed out", "timeout", "time out"]) {
        "The request took too long. Check your connection, then try again."
    } else if has(&["connection refused", "connection reset", "dns", "resolve host", "network is unreachable", "networkerror", "failed to fetch", "error sending request"]) {
        "Bokheim couldn't connect. Check your internet connection, then try again."
    } else if has(&["library's folder is unavailable", "library folder is unavailable"]) {
        "The library folder isn't available. Reconnect its drive or choose an available library, then try again."
    } else {
        fallback
    }
}

/// Wi-Fi failures need recovery steps that work while the device is offline.
pub fn wifi_error_message(diagnostic: &str) -> &'static str {
    log::warn!("Wi-Fi request failed: {diagnostic}");
    let error = diagnostic.to_ascii_lowercase();
    if error.contains("8 to 63") {
        "Enter the network password using 8 to 63 standard keyboard characters."
    } else if error.contains("authentication failed") {
        "The Wi-Fi password wasn't accepted. Check the network password and try again."
    } else if error.contains("security mode") {
        "This network uses a security type Bokheim doesn't support. Try another Wi-Fi network."
    } else if error.contains("selected network is no longer available") {
        "This Wi-Fi network is no longer available. Tap Refresh and choose an available network."
    } else if error.contains("network name must") {
        "Enter a network name. If you've already entered one, try a shorter name."
    } else if error.contains("network scan timed out") {
        "The Wi-Fi scan took too long. Tap Refresh to try again."
    } else if error.contains("network address") {
        "Could not get a network address. Try connecting again, or check your router."
    } else if error.contains("timed out") {
        "Could not connect to Wi-Fi in time. Check the network password and move closer to the router, then try again."
    } else if error.contains("going to sleep") {
        "Wi-Fi is unavailable while your Kobo is going to sleep. Wake the device before trying again."
    } else if error.contains("power control is unavailable") {
        "Bokheim can't switch Wi-Fi on or off on this device. Use the device's Wi-Fi controls instead."
    } else {
        "Could not update Wi-Fi. Try again, or restart Bokheim if the problem continues."
    }
}

/// Account forms share action names across desktop, Android, and the browser.
pub fn authentication_error(diagnostic: &str, action: &str) -> &'static str {
    log::warn!("Account action {action} failed: {diagnostic}");
    let rejected = [400, 401, 409, 422].into_iter().any(|code| is_http_status(diagnostic, code));
    let fallback = match action {
        "login" if rejected => "Could not sign in. Check your email and password, then try again. You can reset your password if you've forgotten it.",
        "verify" if rejected => "Could not verify your email. Check the PIN from your email, or request a new PIN and try again.",
        "reset" if rejected => "Could not reset your password. Request a new reset code and use a password that meets the length requirement.",
        "register" if rejected => "Could not create the account. Check your email and password. If you already have an account, sign in or reset your password.",
        "login" => "Could not sign in. Please try again.",
        "verify" => "Could not verify your email. Please try again.",
        "reset" => "Could not reset your password. Please try again.",
        "register" => "Could not create the account. Please try again.",
        "resend" => "Could not send a verification PIN. Check your email address and try again.",
        "request-reset" => "Could not send a password reset email. Check your email address and try again.",
        _ => "Could not update your account. Please try again.",
    };
    if rejected { fallback } else { friendly_error(diagnostic, fallback) }
}

/// Matches HTTP diagnostics, without treating numbers in paths as status codes.
pub fn is_http_status(diagnostic: &str, code: u16) -> bool {
    let error = diagnostic.to_ascii_lowercase();
    [format!("({code} "), format!("http {code}"), format!("status code: {code}")].iter().any(|prefix| error.match_indices(prefix).any(|(start, _)| error.as_bytes().get(start + prefix.len()).is_none_or(|next| !next.is_ascii_digit())))
}

#[cfg(test)]
mod tests {
    use super::{authentication_error, friendly_error, is_http_status, wifi_error_message};

    #[test]
    fn diagnostics_never_appear_in_user_messages() {
        let fallback = "Could not save this change. Please try again.";
        for diagnostic in ["unexpected worker failure: /home/reader/private", "https://user:secret@example.com/404/503", "database error: token=secret", "", "invalid authentication data: secret"] {
            assert_eq!(friendly_error(diagnostic, fallback), fallback);
        }
    }

    #[test]
    fn recognized_failures_offer_recovery_steps() {
        for (diagnostic, expected) in [
            ("/private/library: No space left on device (os error 28)", "Free up space"),
            ("QuotaExceededError: private", "Free up space"),
            ("Read-only file system (os error 30)", "writable library"),
            ("Permission denied (os error 13): /private", "permission"),
            ("creating sync access handle: private", "Close your other Bokheim tabs"),
            ("not a feed address: private", "complete web address"),
            ("not an OPDS feed: private", "feed address"),
            ("the feed refused this username and password", "Check your username and password"),
            ("HTTP status client error (401 Unauthorized) for url (https://private)", "Sign in again"),
            ("HTTP status client error (403 Forbidden)", "has access"),
            ("HTTP status client error (404 Not Found)", "Refresh the page"),
            ("HTTP status client error (429 Too Many Requests)", "wait a few minutes"),
            ("HTTP status server error (503 Service Unavailable)", "try again later"),
            ("operation timed out: private", "Check your connection"),
            ("error sending request for url (https://private)", "Check your internet connection"),
        ] {
            let message = friendly_error(diagnostic, "Please try again.");
            assert!(message.contains(expected), "{diagnostic}: {message}");
            assert!(!message.contains("private"));
            assert!(!message.contains("os error"));
        }
    }

    #[test]
    fn authentication_recovery_depends_on_the_action() {
        let diagnostic = "HTTP status client error (401 Unauthorized) for url (https://private)";
        assert!(authentication_error(diagnostic, "login").contains("Check your email and password"));
        assert!(authentication_error(diagnostic, "verify").contains("request a new PIN"));
        assert!(authentication_error(diagnostic, "reset").contains("new reset code"));
        assert!(authentication_error("HTTP 429", "login").contains("wait a few minutes"));
        assert!(authentication_error("HTTP 503", "register").contains("try again later"));
        assert!(!authentication_error(diagnostic, "verify").contains("private"));
    }

    #[test]
    fn wifi_recovery_does_not_require_an_existing_internet_connection() {
        assert!(wifi_error_message("Authentication failed. Check the network password.").contains("Check the network password"));
        assert!(wifi_error_message("Network scan timed out. Try Refresh again.").contains("Tap Refresh"));
        assert!(wifi_error_message("Enter a WPA password containing 8 to 63 printable ASCII characters").contains("8 to 63"));
        assert!(wifi_error_message("Obtaining a network address timed out").contains("check your router"));
        let message = wifi_error_message("Failed to open /private/wpa_supplicant: os error 2");
        assert!(!message.contains("private"));
        assert!(message.contains("restart Bokheim"));
    }

    #[test]
    fn http_status_requires_a_status_marker_and_number_boundary() {
        assert!(is_http_status("HTTP 401", 401));
        assert!(is_http_status("status code: 503", 503));
        assert!(!is_http_status("HTTP 4010", 401));
        assert!(!is_http_status("https://example.com/404/503/28", 404));
    }
}
