//! Shared in-memory title filtering for library hierarchy rows.

pub(super) fn normalize_filter(filter: &str) -> String {
    filter.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::normalize_filter;

    #[test]
    fn filter_is_trimmed_and_case_insensitive() {
        assert_eq!(normalize_filter("  Left HAND "), "left hand");
    }
}
