//! Shared byte-size formatting for storage figures shown across the app.

pub(crate) fn format_storage_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1_000.0 && unit < UNITS.len() - 1 {
        value /= 1_000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.2} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn download_byte_progress(downloaded: u64, total: Option<u64>) -> (Option<u8>, String) {
    let received = format_storage_bytes(downloaded);
    match total.filter(|size| *size > 0) {
        Some(total) => (Some(((u128::from(downloaded) * 100 / u128::from(total)).min(100)) as u8), format!("{received} of {}", format_storage_bytes(total))),
        None => (None, format!("{received} downloaded")),
    }
}

#[cfg(test)]
mod tests {
    use super::download_byte_progress;

    #[test]
    fn byte_counts_cover_known_unknown_and_oversized_responses() {
        assert_eq!(download_byte_progress(24_000_000, Some(120_000_000)), (Some(20), "24.0 MB of 120.0 MB".into()));
        assert_eq!(download_byte_progress(1024, None), (None, "1.02 KB downloaded".into()));
        assert_eq!(download_byte_progress(0, Some(0)), (None, "0 B downloaded".into()));
        assert_eq!(download_byte_progress(120, Some(100)), (Some(100), "120 B of 100 B".into()));
        assert_eq!(download_byte_progress(u64::MAX, Some(u64::MAX)).0, Some(100));
    }
}
