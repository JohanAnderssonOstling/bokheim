//! Fixtures whose values are meaningful to both sync clients and servers.

use sync_common::ContentHash;

pub fn fixture_content_hash(value: u64) -> ContentHash {
    ContentHash::new(&format!("{value:064x}"))
}
