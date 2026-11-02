//! Actionable messages for book downloads and imports, worded from how the
//! transfer failed. Diagnostics stay in the logs where they arose.

use operation_failure::Failure;

pub fn book_transfer_error_message(failure: Option<Failure>) -> &'static str {
    match failure {
        Some(Failure::DrmProtected) => "DRM-protected downloads aren't supported.",
        Some(Failure::StorageFull) => "Not enough storage to save this book. Free up space on the library's device or drive, then retry.",
        Some(Failure::CloudStorageFull) => "Cloud storage is full. Free up account space and retry. Your local books are kept.",
        Some(Failure::ReadOnlyStorage) => "Your library is read-only. Allow changes or choose a writable library folder.",
        Some(Failure::PermissionDenied) => "Couldn't save to your library. Check its folder permissions or choose another library.",
        Some(Failure::NotFound) => "The download link is no longer available. Try another edition or the source website.",
        Some(Failure::Unauthorized | Failure::SignInRequired | Failure::CatalogCredentialsRejected) => "Sign in to the catalog, then retry the download.",
        Some(Failure::Forbidden) => "The source blocked this download. Try its website or another edition.",
        Some(Failure::RateLimited | Failure::ServiceUnavailable) => "Download service unavailable. Try again later.",
        Some(Failure::TimedOut) => "Download timed out. Check your internet connection and retry.",
        Some(Failure::Offline) => "Download couldn't finish. Check your internet connection and retry.",
        Some(Failure::UnreadableBook | Failure::NotACatalog) => "Couldn't open the book file. Download it again or try another edition.",
        Some(Failure::LibraryFolderUnavailable) => "Library folder unavailable. Reconnect its drive or choose another library.",
        Some(Failure::ClientUpgradeRequired) => "This client and server are incompatible. Reload the web app or check for app updates, then retry. If it persists, report the problem.",
        _ => "Couldn't download or save the book. Retry or try another edition.",
    }
}
