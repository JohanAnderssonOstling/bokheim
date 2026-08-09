use identifiers::ContentHash;

pub fn fixture_content_hash(value: u64) -> ContentHash {
    ContentHash::new(&format!("{value:064x}"))
}
