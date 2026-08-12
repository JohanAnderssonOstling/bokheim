//! Content-addressed identities shared by storage models and transfer protocols.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 64]);

impl ContentHash {
    pub fn new(value: &str) -> Self {
        let mut bytes = [0_u8; 64];
        bytes.copy_from_slice(value.as_bytes());
        Self(bytes)
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("ContentHash is generated as ASCII hexadecimal text")
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

impl serde::Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ContentHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::new(&<String as serde::Deserialize>::deserialize(deserializer)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_the_canonical_hash_text() {
        let value = "a".repeat(64);
        let hash = ContentHash::new(&value);
        assert_eq!(hash.as_str(), value);
        assert_eq!(hash.to_string(), value);
    }
}
