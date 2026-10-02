//! Transfer deadlines and progress notification policy.

pub const SMALL_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
pub const BOOK_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

// Allow a minute for setup plus transfer time at 64 KiB/s. A fixed short
// timeout would repeatedly abort large audiobooks even on a working link.
pub fn upload_timeout(bytes: u64) -> std::time::Duration {
    std::time::Duration::from_secs(60 + bytes.div_ceil(64 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn upload_deadline_scales_with_book_size() {
        assert_eq!(upload_timeout(0), Duration::from_secs(60));
        assert_eq!(upload_timeout(1), Duration::from_secs(61));
        assert_eq!(upload_timeout(64 * 1024), Duration::from_secs(61));
        assert_eq!(upload_timeout(1024 * 1024 * 1024), Duration::from_secs(60 + 16384));
    }
}
