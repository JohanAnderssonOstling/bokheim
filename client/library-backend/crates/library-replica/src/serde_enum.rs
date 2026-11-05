//! Read accepted enum names from Serde's deserialization errors, honoring renames.

use serde::de;
use std::fmt::Display;

#[derive(Debug)]
pub(crate) struct Names(pub(crate) &'static [&'static str]);
impl Display for Names {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("enum variant names")
    }
}
impl std::error::Error for Names {}
impl de::Error for Names {
    fn custom<T: Display>(_: T) -> Self {
        Self(&[])
    }
    fn unknown_variant(_: &str, expected: &'static [&'static str]) -> Self {
        Self(expected)
    }
}
