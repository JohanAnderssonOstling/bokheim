//! Reader interoperability contracts shared by library import and annotation reconciliation.
mod annotations;
pub use annotations::{annotation_id, decode_annotation, decode_annotation_identity, readest_color, ReaderFormat};
#[cfg(not(target_arch = "wasm32"))]
pub mod profiles;

/// JavaScript encodeURIComponent, used by Foliate's JSONStorage filenames.
pub fn encoded_identifier(identifier: &str) -> String {
    let mut encoded = String::new();
    for byte in identifier.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}
