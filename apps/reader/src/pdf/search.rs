//! PDF search query policy.

/// Searching starts at two Unicode characters. Shorter input still produces
/// an empty query so old results and in-flight work are cleared.
pub(super) fn renderer_query(input: &str) -> String {
    if input.chars().count() >= 2 { input.to_owned() } else { String::new() }
}
