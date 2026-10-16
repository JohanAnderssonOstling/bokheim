//! Safe, actionable messages for book downloads and imports. Transport errors
//! currently arrive as strings; never display an unrecognized diagnostic.

const DRM: &str = "This file is not supported. Bokheim cannot read DRM-protected downloads.";
const NO_SPACE: &str = "There isn't enough free space to save this book. Free up some space on the device or drive that holds your library, then try again.";
const READ_ONLY: &str = "Your library is read-only. Choose a writable library location or allow changes to it, then try again.";
const PERMISSION: &str = "Bokheim can't save files in your library. Check the folder's permissions or choose another library location, then try again.";
const UNAVAILABLE: &str = "This book is no longer available at its download link. Try another edition or check the source website.";
const SIGN_IN: &str = "The source requires you to sign in. Sign in to the catalog, then try again.";
const BLOCKED: &str = "The source is blocking this download. Try downloading from its website or choose another edition.";
const SERVICE: &str = "The download service is temporarily unavailable. Please try again later.";
const TIMEOUT: &str = "The download took too long. Check your internet connection, then try again.";
const NETWORK: &str = "The download couldn't finish. Check your internet connection, then try again.";
const FILE: &str = "This book's file couldn't be opened. Try downloading it again or choose another edition.";
const FALLBACK: &str = "The book couldn't be downloaded or saved. Please try again. If it keeps failing, try another edition.";

pub fn book_transfer_error_message(diagnostic: &str) -> &'static str {
    // Catalog imports may already have been translated before reaching the
    // shared notification layer. Only these exact application-owned messages
    // may pass through unchanged.
    if let Some(message) = [NO_SPACE, READ_ONLY, PERMISSION, UNAVAILABLE, SIGN_IN, BLOCKED, SERVICE, TIMEOUT, NETWORK, FILE, FALLBACK, DRM].into_iter().find(|message| *message == diagnostic) {
        return message;
    }
    log::warn!("Book transfer failed: {diagnostic}");
    let error = diagnostic.to_ascii_lowercase();
    let has = |terms: &[&str]| terms.iter().any(|term| error.contains(term));
    let http_status = |code| crate::is_http_status(&error, code);

    if has(&["drm-protected", "drm protected"]) {
        DRM
    } else if has(&["no space left", "os error 28", "os error 112", "disk full", "database or disk is full", "disk quota exceeded", "not enough storage", "not enough space", "quotaexceedederror"]) {
        NO_SPACE
    } else if has(&["read-only file system", "readonly filesystem", "readonly database", "read-only database", "os error 30"]) {
        READ_ONLY
    } else if has(&["permission denied", "access is denied", "os error 13"]) {
        PERMISSION
    } else if http_status(404) || http_status(410) || has(&["not in the catalog", "not offered for download", "no longer listed"]) {
        UNAVAILABLE
    } else if http_status(401) || has(&["sign in", "sign-in", "credentials"]) {
        SIGN_IN
    } else if http_status(403) || has(&["blocking access", "browser verification"]) {
        BLOCKED
    } else if http_status(429) || (500..=599).any(http_status) {
        SERVICE
    } else if has(&["timed out", "timeout", "time out"]) {
        TIMEOUT
    } else if has(&["connection", "dns", "resolve host", "network", "error sending request", "error decoding response body"]) {
        NETWORK
    } else if has(&["could not read the downloaded audiobook", "invalid zip", "invalid archive", "unsupported", "not supported", "corrupt", "differs from the catalog", "book is empty"]) {
        FILE
    } else {
        FALLBACK
    }
}

#[cfg(test)]
mod tests {
    use super::book_transfer_error_message;

    #[test]
    fn storage_failures_explain_how_to_recover_without_showing_paths_or_codes() {
        for diagnostic in [
            "could not write the imported book in library folder '/home/reader/private': No space left on device (os error 28)",
            "There is not enough space on the disk. (os error 112)",
            "database or disk is full",
            "Disk quota exceeded (os error 122)",
            "QuotaExceededError: The quota has been exceeded.",
        ] {
            let message = book_transfer_error_message(diagnostic);
            assert!(message.contains("Free up some space"), "{diagnostic}: {message}");
            assert!(!message.contains("/home/"));
            assert!(!message.contains("os error"));
        }
    }

    #[test]
    fn download_failures_give_specific_next_steps() {
        for (diagnostic, expected) in [
            ("HTTP status client error (404 Not Found) for url (https://archive.org/private)", "Try another edition"),
            ("HTTP status client error (410 Gone)", "Try another edition"),
            ("HTTP status client error (401 Unauthorized)", "Sign in"),
            ("HTTP status client error (403 Forbidden)", "source is blocking"),
            ("HTTP status client error (429 Too Many Requests)", "try again later"),
            ("HTTP status server error (503 Service Unavailable)", "try again later"),
            ("operation timed out", "took too long"),
            ("error sending request for url (https://archive.org/private)", "Check your internet connection"),
            ("could not read the downloaded audiobook: invalid Zip archive", "file couldn't be opened"),
            ("Permission denied (os error 13)", "folder's permissions"),
            ("Read-only file system (os error 30)", "read-only"),
        ] {
            let message = book_transfer_error_message(diagnostic);
            assert!(message.contains(expected), "{diagnostic}: {message}");
            assert!(!message.contains("https://"));
            assert!(!message.contains("os error"));
        }
    }

    #[test]
    fn presentation_is_idempotent_and_preserves_supported_recovery_messages() {
        for diagnostic in ["no space left", "read-only file system", "permission denied", "HTTP 404", "HTTP 401", "HTTP 403", "HTTP 503", "timed out", "error sending request", "invalid archive", "unknown", "DRM-protected"] {
            let message = book_transfer_error_message(diagnostic);
            assert_eq!(book_transfer_error_message(message), message);
        }
    }

    #[test]
    fn unknown_errors_never_expose_diagnostics_and_url_numbers_are_not_status_codes() {
        let fallback = book_transfer_error_message("unexpected backend exception: /home/reader/private");
        assert_eq!(fallback, book_transfer_error_message("could not load https://example.com/404/503/28"));
        assert!(fallback.contains("Please try again"));
        assert!(!fallback.contains("backend"));
        assert!(!fallback.contains("/home/"));
    }
}
