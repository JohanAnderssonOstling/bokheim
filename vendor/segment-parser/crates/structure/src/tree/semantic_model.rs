use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderSemantic {
    key: String,
    canonical: String,
}

impl PlaceholderSemantic {
    pub fn new(key: String, canonical: String) -> Self { Self { key, canonical } }
    pub fn key(&self) -> &str { &self.key }
    pub fn canonical(&self) -> &str { &self.canonical }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VersionPrefixStyle {
    None,
    LowerV,
    UpperV,
}

impl VersionPrefixStyle {
    pub fn text(self) -> &'static str { match self { Self::None => "", Self::LowerV => "v", Self::UpperV => "V" } }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionSemantic {
    prefix_style: VersionPrefixStyle,
    components: Box<[String]>,
    prerelease: Option<String>,
    build_metadata: Option<String>,
    canonical: String,
}

impl VersionSemantic {
    pub fn new(prefix_style: VersionPrefixStyle, components: Vec<String>) -> Self {
        let mut semantic = Self { prefix_style, components: components.into_boxed_slice(), prerelease: None, build_metadata: None, canonical: String::new() };
        semantic.rebuild_canonical();
        semantic
    }
    pub fn with_prerelease(mut self, prerelease: Option<String>) -> Self { self.prerelease = prerelease; self.rebuild_canonical(); self }
    pub fn with_build_metadata(mut self, build_metadata: Option<String>) -> Self { self.build_metadata = build_metadata; self.rebuild_canonical(); self }
    pub fn prefix_style(&self) -> VersionPrefixStyle { self.prefix_style }
    pub fn components(&self) -> &[String] { &self.components }
    pub fn prerelease(&self) -> Option<&str> { self.prerelease.as_deref() }
    pub fn build_metadata(&self) -> Option<&str> { self.build_metadata.as_deref() }
    pub fn canonical(&self) -> &str { &self.canonical }
    fn rebuild_canonical(&mut self) {
        self.canonical = self.components.join(".");
        if let Some(prerelease) = &self.prerelease {
            self.canonical.push('-');
            self.canonical.push_str(prerelease);
        }
        if let Some(build_metadata) = &self.build_metadata {
            self.canonical.push('+');
            self.canonical.push_str(build_metadata);
        }
    }
}

/// Language-independent part of speech following the Universal POS tagset.
/// Parser-specific tagset conversion lives in `segment-parser`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PartOfSpeech {
    Adj,
    Adp,
    Adv,
    Aux,
    Cconj,
    Det,
    Intj,
    Noun,
    Num,
    Part,
    Pron,
    Propn,
    Punct,
    Sconj,
    Sym,
    Verb,
    /// Other / unclassifiable (nlprule `UNKNOWN`, foreign material).
    X,
}
