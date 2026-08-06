use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU64;
use std::ops::Deref;
use std::str::FromStr;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
use uuid::Uuid;

pub use bokheim_algorithms::{audiobook_position_millis, audiobook_reading_position, pdf_page_from_reading_position, pdf_page_reading_position, valid_epub_cfi, valid_reading_position};

mod contributor_roles;
pub use contributor_roles::{ContentRole, ContributorRole, InvalidMarcRelatorCode, MarcRelatorCode, PerformanceRole, ProductionRole, ProvenanceRole, SupplementaryWriterRole, SupportRole, VisualRole};

pub mod api;
pub use bokheim_wire as wire;

/// Maximum encoded size of a synchronized JPEG cover.
pub const MAX_THUMBNAIL_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum width or height of a synchronized cover.
pub const MAX_THUMBNAIL_DIMENSION: u32 = 2048;
/// Maximum decoded pixel count of a synchronized cover.
pub const MAX_THUMBNAIL_PIXELS: u64 = 4 * 1024 * 1024;
/// Maximum number of synchronization mutations returned by one pull response.
/// Clients drain additional pages using the explicit library revision cursor.
pub const MAX_PULL_CHANGES: usize = 1_000;
/// Maximum mutations accepted in one push/exchange page. Clients checkpoint
/// larger durable outboxes page by page instead of creating unbounded database
/// transactions or exceeding the HTTP body limit.
pub const MAX_PUSH_MUTATIONS: usize = 1_000;
/// Target maximum encoded mutation bytes selected by clients for one page.
/// The HTTP request has additional envelope overhead and an 8 MiB hard limit.
pub const MAX_PUSH_MUTATION_BYTES: usize = 4 * 1024 * 1024;
/// Normal clock skew is harmless. A mutation further ahead than this is most
/// likely produced by a misconfigured device and must not pin an LWW register.
pub const MAX_LWW_FUTURE_SKEW_MS: u64 = 5 * 60 * 1_000;

/// A non-negative Unix timestamp measured in milliseconds.
///
/// Synchronization uses this type for every wall-clock value so independent
/// clients cannot accidentally mix seconds, milliseconds, and microseconds.
/// The upper bound matches the signed PostgreSQL and SQLite integer storage
/// used by the protocol implementations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct UnixMillis(u64);

impl UnixMillis {
    pub const MAX: u64 = i64::MAX as u64;

    pub const fn new(value: u64) -> Result<Self, TimestampError> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(TimestampError)
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn storage_i64(self) -> i64 {
        self.0 as i64
    }

    pub const fn saturating_add(self, milliseconds: u64) -> Self {
        let value = self.0.saturating_add(milliseconds);
        if value > Self::MAX {
            Self(Self::MAX)
        } else {
            Self(value)
        }
    }
}

impl fmt::Display for UnixMillis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for UnixMillis {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimestampError;

impl fmt::Display for TimestampError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Unix-millisecond timestamps must fit signed 64-bit storage")
    }
}

impl std::error::Error for TimestampError {}

/// Stable logical identity of a book.
///
/// The inner representation is deliberately private. Every boundary uses the
/// canonical string produced and parsed by this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BookId(u64);

impl BookId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn canonical(self) -> String {
        self.0.to_string()
    }

    pub fn parse(value: &str) -> Result<Self, std::num::ParseIntError> {
        value.parse::<u64>().map(Self)
    }

    /// Derives the catalog id from the first eight bytes of a full blake3 hash.
    pub fn from_content_hash(content_hash: &ContentHash) -> Self {
        let bytes = content_hash.as_str().as_bytes();
        let mut id_bytes = [0_u8; 8];
        for (index, byte) in id_bytes.iter_mut().enumerate() {
            let high = decode_hex_nibble(bytes[index * 2]);
            let low = decode_hex_nibble(bytes[index * 2 + 1]);
            *byte = (high << 4) | low;
        }
        Self(u64::from_le_bytes(id_bytes))
    }
}

impl fmt::Display for BookId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for BookId {
    type Err = std::num::ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for BookId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.canonical())
        } else {
            serializer.serialize_u64(self.0)
        }
    }
}

impl<'de> Deserialize<'de> for BookId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            return u64::deserialize(deserializer).map(Self::new);
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CompatibleBookId {
            String(String),
            Number(u64),
        }

        match CompatibleBookId::deserialize(deserializer)? {
            CompatibleBookId::String(value) => Self::parse(&value).map_err(serde::de::Error::custom),
            CompatibleBookId::Number(value) => Ok(Self::new(value)),
        }
    }
}

/// Full-file blake3 hex digest used as the content-addressed blob key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentHashParseError;

impl fmt::Display for ContentHashParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("content hash must be exactly 64 lowercase hexadecimal characters")
    }
}

impl std::error::Error for ContentHashParseError {}

impl ContentHash {
    pub fn parse(value: &str) -> Result<Self, ContentHashParseError> {
        if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            Ok(Self(value.to_owned()))
        } else {
            Err(ContentHashParseError)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for ContentHash {
    type Err = ContentHashParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return serializer.serialize_str(self.as_str());
        }
        let encoded = self.as_str().as_bytes();
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (decode_hex_nibble(encoded[index * 2]) << 4) | decode_hex_nibble(encoded[index * 2 + 1]);
        }
        bytes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            let bytes = <[u8; 32]>::deserialize(deserializer)?;
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut encoded = String::with_capacity(64);
            for byte in bytes {
                encoded.push(HEX[(byte >> 4) as usize] as char);
                encoded.push(HEX[(byte & 0x0f) as usize] as char);
            }
            return Ok(Self(encoded));
        }
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainTextError {
    kind: &'static str,
}

impl fmt::Display for DomainTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} must be non-empty, bounded text without control characters or path separators", self.kind)
    }
}

impl std::error::Error for DomainTextError {}

fn validate_domain_text(value: &str, kind: &'static str) -> Result<(), DomainTextError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(|character| character.is_control() || character == '/' || character == '\\') {
        Err(DomainTextError { kind })
    } else {
        Ok(())
    }
}

macro_rules! domain_text_type {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, DomainTextError> {
                validate_domain_text(value, $kind)?;
                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = DomainTextError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

domain_text_type!(ClientId, "replica id");
domain_text_type!(LibraryId, "library id");
domain_text_type!(DirId, "directory id");

/// Globally unique identity of one logical mutation. Unlike an actor-local
/// sequence this remains unique when a library database is copied and both
/// copies subsequently produce changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MutationId(Uuid);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MutationIdError;

impl fmt::Display for MutationIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("mutation id must be a non-nil UUID")
    }
}

impl std::error::Error for MutationIdError {}

impl MutationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, MutationIdError> {
        let value = Uuid::parse_str(value).map_err(|_| MutationIdError)?;
        if value.is_nil() {
            Err(MutationIdError)
        } else {
            Ok(Self(value))
        }
    }
}

impl Default for MutationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MutationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for MutationId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            self.0.into_bytes().serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for MutationId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            let value = Uuid::from_bytes(<[u8; 16]>::deserialize(deserializer)?);
            return if value.is_nil() { Err(serde::de::Error::custom(MutationIdError)) } else { Ok(Self(value)) };
        }
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileName(String);

impl FileName {
    pub fn parse(value: &str) -> Result<Self, DomainTextError> {
        validate_domain_text(value, "file name")?;
        if value == "." || value == ".." {
            return Err(DomainTextError { kind: "file name" });
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for FileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for FileName {
    type Err = DomainTextError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for FileName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FileName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

fn decode_hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("ContentHash guarantees lowercase hexadecimal bytes"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryState {
    Present { parent_id: DirId, name: FileName },
    Deleted { parent_id: DirId, name: FileName },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStyle {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, specta::Type)]
pub struct PdfAnnotationRect {
    #[specta(type = specta_typescript::Number)]
    pub left: f32,
    #[specta(type = specta_typescript::Number)]
    pub top: f32,
    #[specta(type = specta_typescript::Number)]
    pub width: f32,
    #[specta(type = specta_typescript::Number)]
    pub height: f32,
}

impl PdfAnnotationRect {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Option<Self> {
        let rect = Self { left, top, width, height };
        rect.is_valid().then_some(rect)
    }

    pub fn is_valid(self) -> bool {
        [self.left, self.top, self.width, self.height].into_iter().all(f32::is_finite)
            && self.left >= 0.0
            && self.top >= 0.0
            && self.width > 0.0
            && self.height > 0.0
            && self.left + self.width <= 1.000_01
            && self.top + self.height <= 1.000_01
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, specta::Type)]
pub struct PdfAnnotationAnchor {
    #[specta(type = specta_typescript::Number)]
    pub page_index: u32,
    pub rects: Vec<PdfAnnotationRect>,
    pub fallback_cfi: Option<String>,
}

impl PdfAnnotationAnchor {
    pub fn new(page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String>) -> Option<Self> {
        let fallback_valid = fallback_cfi.as_deref().is_none_or(valid_epub_cfi);
        (!rects.is_empty() && rects.len() <= 4096 && rects.iter().all(|rect| rect.is_valid()) && fallback_valid).then_some(Self { page_index, rects, fallback_cfi })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationAnchor {
    EpubCfi {
        cfi: String,
    },
    Pdf {
        #[specta(type = specta_typescript::Number)]
        page_index: u32,
        rects: Vec<PdfAnnotationRect>,
        fallback_cfi: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "AnnotationAnchor")]
enum AnnotationAnchorBinary {
    EpubCfi { cfi: String },
    Pdf { page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String> },
}

mod annotation_anchor_binary {
    use super::{AnnotationAnchor, AnnotationAnchorBinary};

    pub fn serialize<S: serde::Serializer>(value: &AnnotationAnchor, serializer: S) -> Result<S::Ok, S::Error> {
        AnnotationAnchorBinary::serialize(value, serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<AnnotationAnchor, D::Error> {
        AnnotationAnchorBinary::deserialize(deserializer)
    }
}

impl AnnotationAnchor {
    pub fn epub_cfi(cfi: impl Into<String>) -> Option<Self> {
        let cfi = cfi.into();
        valid_epub_cfi(&cfi).then_some(Self::EpubCfi { cfi })
    }

    pub fn pdf(anchor: PdfAnnotationAnchor) -> Option<Self> {
        PdfAnnotationAnchor::new(anchor.page_index, anchor.rects, anchor.fallback_cfi).map(|anchor| Self::Pdf { page_index: anchor.page_index, rects: anchor.rects, fallback_cfi: anchor.fallback_cfi })
    }

    pub fn epub_cfi_value(&self) -> Option<&str> {
        match self {
            Self::EpubCfi { cfi } => Some(cfi),
            Self::Pdf { fallback_cfi, .. } => fallback_cfi.as_deref(),
        }
    }

    pub fn pdf_anchor(&self) -> Option<PdfAnnotationAnchor> {
        match self {
            Self::EpubCfi { .. } => None,
            Self::Pdf { page_index, rects, fallback_cfi } => PdfAnnotationAnchor::new(*page_index, rects.clone(), fallback_cfi.clone()),
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::EpubCfi { cfi } => valid_epub_cfi(cfi),
            Self::Pdf { .. } => self.pdf_anchor().is_some(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationState {
    pub book_id: BookId,
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "AnnotationState")]
struct AnnotationStateHuman {
    pub book_id: BookId,
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "AnnotationState")]
struct AnnotationStateBinary {
    pub book_id: BookId,
    #[serde(with = "annotation_anchor_binary")]
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

impl Serialize for AnnotationState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            AnnotationStateHuman::serialize(self, serializer)
        } else {
            AnnotationStateBinary::serialize(self, serializer)
        }
    }
}

impl<'de> Deserialize<'de> for AnnotationState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            AnnotationStateHuman::deserialize(deserializer)
        } else {
            AnnotationStateBinary::deserialize(deserializer)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookmarkState {
    pub book_id: BookId,
    pub cfi: String,
    pub label: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

/// A user-authored, directed semantic edge between two books.
///
/// `relation` is intentionally user-defined rather than a closed enum: a
/// reader can express links such as "responds to", "expands", or "contrasts
/// with" without requiring a protocol migration. The stable link id gives the
/// whole record LWW semantics, including deletion through a tombstone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookLinkState {
    pub source_book_id: BookId,
    pub target_book_id: BookId,
    pub relation: String,
    pub note: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

impl DirectoryState {
    pub fn parent_id(&self) -> &DirId {
        match self {
            Self::Present { parent_id, .. } | Self::Deleted { parent_id, .. } => parent_id,
        }
    }

    pub fn name(&self) -> &FileName {
        match self {
            Self::Present { name, .. } | Self::Deleted { name, .. } => name,
        }
    }

    pub const fn is_deleted(&self) -> bool {
        matches!(self, Self::Deleted { .. })
    }
}

/// Stable Bokheim identity for one provisional or externally-resolved person
/// or organization credited by a publication. It is generated offline and
/// remains independent of any particular external authority identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentId(Uuid);

impl AgentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, AgentIdError> {
        let value = Uuid::parse_str(value).map_err(|_| AgentIdError)?;
        if value.is_nil() {
            Err(AgentIdError)
        } else {
            Ok(Self(value))
        }
    }

    /// Deterministic bridge for assigning IDs while migrating credits written
    /// before stable agent identities existed. New agents must use `new`.
    pub fn from_migration_seed(seed: &[u8]) -> Self {
        const NAMESPACE: Uuid = Uuid::from_bytes([0x7f, 0xe7, 0x7e, 0x34, 0x29, 0x7f, 0x4a, 0xfa, 0x99, 0x8d, 0x2f, 0x86, 0xf0, 0x58, 0x7b, 0x31]);
        Self(Uuid::new_v5(&NAMESPACE, seed))
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for AgentId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            self.0.into_bytes().serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for AgentId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            let value = Uuid::from_bytes(<[u8; 16]>::deserialize(deserializer)?);
            return if value.is_nil() { Err(serde::de::Error::custom(AgentIdError)) } else { Ok(Self(value)) };
        }
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentIdError;

impl fmt::Display for AgentIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("agent id must be a non-nil UUID")
    }
}

impl std::error::Error for AgentIdError {}

/// Source-compatible names used where a role-specific identity communicates
/// intent. All publication agents share the same underlying identity domain.
pub type AuthorId = AgentId;
pub type PublisherId = AgentId;
pub type ContributorId = AgentId;
pub type AuthorIdError = AgentIdError;

/// One ordered creator credit exactly as a publication presents it. The ID
/// selects Bokheim's author entity while the names remain the publication's
/// credited text. External authority resolution is a separate projection.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthorCredit {
    author_id: AgentId,
    name: String,
    sort_name: Option<String>,
}

impl AuthorCredit {
    pub fn new(name: impl Into<String>, sort_name: Option<String>) -> Result<Self, AuthorCreditError> {
        Self::with_id(AuthorId::new(), name, sort_name)
    }

    pub fn with_id(author_id: AgentId, name: impl Into<String>, sort_name: Option<String>) -> Result<Self, AuthorCreditError> {
        let name = name.into();
        let name = name.trim();
        if !valid_author_credit_text(name) {
            return Err(AuthorCreditError);
        }
        let sort_name = sort_name.map(|value| value.trim().to_owned()).map(|value| valid_author_credit_text(&value).then_some(value).ok_or(AuthorCreditError)).transpose()?;
        Ok(Self { author_id, name: name.to_owned(), sort_name })
    }

    pub fn from_name(name: impl Into<String>) -> Result<Self, AuthorCreditError> {
        Self::new(name, None)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn author_id(&self) -> AuthorId {
        self.author_id
    }

    pub const fn agent_id(&self) -> AgentId {
        self.author_id
    }

    pub fn sort_name(&self) -> Option<&str> {
        self.sort_name.as_deref()
    }

    pub fn reidentified(&self, author_id: AgentId) -> Self {
        Self { author_id, name: self.name.clone(), sort_name: self.sort_name.clone() }
    }

    fn encoded_text_len(&self) -> usize {
        self.name.len() + self.sort_name.as_ref().map_or(0, String::len)
    }
}

impl<'de> Deserialize<'de> for AuthorCredit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedAuthorCredit {
            author_id: AgentId,
            name: String,
            #[serde(default)]
            sort_name: Option<String>,
        }

        let encoded = EncodedAuthorCredit::deserialize(deserializer)?;
        Self::with_id(encoded.author_id, encoded.name, encoded.sort_name).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorCreditError;

impl fmt::Display for AuthorCreditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("author credits must contain bounded non-empty names without control characters")
    }
}

impl std::error::Error for AuthorCreditError {}

fn valid_author_credit_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

/// A publisher spelling exactly as credited by a publication. Equality is
/// deliberately distinct from publisher identity; [`PublisherName::match_key`]
/// supplies the conservative grouping key used by library indexes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PublisherName(String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PublisherMatchKey(String);

impl PublisherName {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        normalized_publication_text(value.as_ref()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn match_key(&self) -> PublisherMatchKey {
        let normalized = self.0.nfkc().collect::<String>();
        let mut key = String::with_capacity(normalized.len());
        let mut separator_pending = false;
        for character in normalized.chars() {
            if character.is_alphanumeric() || is_combining_mark(character) {
                if separator_pending && !key.is_empty() {
                    key.push(' ');
                }
                separator_pending = false;
                key.extend(character.to_lowercase());
            } else {
                separator_pending = true;
            }
        }
        PublisherMatchKey(key)
    }
}

impl PublisherMatchKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PublisherName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// One exact publisher credit linked to a stable Bokheim agent. The credited
/// spelling remains publication evidence; identity resolution can later add
/// authority identifiers without rewriting that evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublisherCredit {
    publisher_id: PublisherId,
    name: PublisherName,
}

impl PublisherCredit {
    pub fn new(name: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        Self::with_id(PublisherId::new(), name)
    }

    pub fn with_id(publisher_id: PublisherId, name: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        Ok(Self { publisher_id, name: PublisherName::parse(name)? })
    }

    pub const fn publisher_id(&self) -> PublisherId {
        self.publisher_id
    }

    pub const fn agent_id(&self) -> AgentId {
        self.publisher_id
    }

    pub fn name(&self) -> &PublisherName {
        &self.name
    }
}

impl<'de> Deserialize<'de> for PublisherCredit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublisherCredit {
            publisher_id: PublisherId,
            name: PublisherName,
        }

        let encoded = EncodedPublisherCredit::deserialize(deserializer)?;
        Self::with_id(encoded.publisher_id, encoded.name.as_str()).map_err(serde::de::Error::custom)
    }
}

/// Canonical, case-insensitive representation of an EPUB language tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct LanguageTag(String);

impl LanguageTag {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        let value = value.as_ref().trim();
        let syntactically_valid = !value.is_empty()
            && value.len() <= 63
            && value.split('-').all(|part| !part.is_empty() && part.len() <= 8 && part.chars().all(|character| character.is_ascii_alphanumeric()))
            && value.split('-').next().is_some_and(|part| ((2..=8).contains(&part.len()) || part.eq_ignore_ascii_case("x")) && part.chars().all(|character| character.is_ascii_alphabetic()));
        if !syntactically_valid {
            return Err(PublicationMetadataError);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for LanguageTag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationIdentifierScheme {
    Unspecified,
    Isbn,
    Asin,
    Doi,
    Issn,
    Oclc,
    Lccn,
    Uuid,
    Calibre,
    Google,
    Goodreads,
    Kobo,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationIdentifierType {
    value: String,
    scheme: Option<String>,
}

impl PublicationIdentifierType {
    pub fn new(value: impl AsRef<str>, scheme: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self { value: bounded_publication_text(value.as_ref())?, scheme: scheme.map(|value| bounded_publication_text(&value)).transpose()? })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationIdentifierType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublicationIdentifierType {
            value: String,
            #[serde(default)]
            scheme: Option<String>,
        }

        let encoded = EncodedPublicationIdentifierType::deserialize(deserializer)?;
        Self::new(encoded.value, encoded.scheme).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationIdentifier {
    value: String,
    canonical_value: Option<String>,
    scheme: PublicationIdentifierScheme,
    identifier_types: Vec<PublicationIdentifierType>,
}

impl PublicationIdentifier {
    pub fn new(value: impl AsRef<str>, scheme: PublicationIdentifierScheme, identifier_types: Vec<PublicationIdentifierType>) -> Result<Self, PublicationMetadataError> {
        let value = bounded_publication_text(value.as_ref())?;
        if identifier_types.len() > 32 {
            return Err(PublicationMetadataError);
        }
        let canonical_value = canonical_identifier_value(&scheme, &value);
        Ok(Self { value, canonical_value, scheme, identifier_types })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn canonical_value(&self) -> Option<&str> {
        self.canonical_value.as_deref()
    }

    pub fn lookup_value(&self) -> &str {
        self.canonical_value.as_deref().unwrap_or(&self.value)
    }

    pub fn scheme(&self) -> &PublicationIdentifierScheme {
        &self.scheme
    }

    pub fn identifier_types(&self) -> &[PublicationIdentifierType] {
        &self.identifier_types
    }
}

impl<'de> Deserialize<'de> for PublicationIdentifier {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublicationIdentifier {
            value: String,
            canonical_value: Option<String>,
            scheme: PublicationIdentifierScheme,
            #[serde(default)]
            identifier_types: Vec<PublicationIdentifierType>,
        }

        let encoded = EncodedPublicationIdentifier::deserialize(deserializer)?;
        let decoded = Self::new(encoded.value, encoded.scheme, encoded.identifier_types).map_err(serde::de::Error::custom)?;
        if encoded.canonical_value != decoded.canonical_value {
            return Err(serde::de::Error::custom("publication identifier canonical value does not match its source value"));
        }
        Ok(decoded)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationContributor {
    contributor_id: ContributorId,
    name: String,
    sort_name: Option<String>,
    role: MarcRelatorCode,
}

impl PublicationContributor {
    pub fn new(name: impl AsRef<str>, sort_name: Option<String>, role: MarcRelatorCode) -> Result<Self, PublicationMetadataError> {
        Self::with_id(ContributorId::new(), name, sort_name, role)
    }

    pub fn with_id(contributor_id: ContributorId, name: impl AsRef<str>, sort_name: Option<String>, role: MarcRelatorCode) -> Result<Self, PublicationMetadataError> {
        Ok(Self { contributor_id, name: normalized_publication_text(name.as_ref())?, sort_name: sort_name.map(|value| normalized_publication_text(&value)).transpose()?, role })
    }

    pub const fn contributor_id(&self) -> ContributorId {
        self.contributor_id
    }

    pub const fn agent_id(&self) -> AgentId {
        self.contributor_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sort_name(&self) -> Option<&str> {
        self.sort_name.as_deref()
    }

    pub const fn role_code(&self) -> MarcRelatorCode {
        self.role
    }

    pub const fn role(&self) -> ContributorRole {
        self.role.role()
    }
}

impl<'de> Deserialize<'de> for PublicationContributor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublicationContributor {
            contributor_id: ContributorId,
            name: String,
            sort_name: Option<String>,
            role: MarcRelatorCode,
        }

        let encoded = EncodedPublicationContributor::deserialize(deserializer)?;
        Self::with_id(encoded.contributor_id, encoded.name, encoded.sort_name, encoded.role).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialDate {
    year: i32,
    month: Option<u8>,
    day: Option<u8>,
}

impl PartialDate {
    pub const fn year(self) -> i32 {
        self.year
    }

    pub const fn month(self) -> Option<u8> {
        self.month
    }

    pub const fn day(self) -> Option<u8> {
        self.day
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationDate {
    id: Option<String>,
    value: String,
    normalized: Option<PartialDate>,
    event: Option<String>,
}

impl PublicationDate {
    pub fn new(id: Option<String>, value: impl AsRef<str>, event: Option<String>) -> Result<Self, PublicationMetadataError> {
        let id = id.map(|value| bounded_publication_text(&value)).transpose()?;
        let value = bounded_publication_text(value.as_ref())?;
        let normalized = parse_partial_date(&value);
        let event = event.map(|value| bounded_publication_text(&value)).transpose()?;
        Ok(Self { id, value, normalized, event })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub const fn normalized(&self) -> Option<PartialDate> {
        self.normalized
    }

    pub fn event(&self) -> Option<&str> {
        self.event.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationDate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublicationDate {
            id: Option<String>,
            value: String,
            normalized: Option<PartialDate>,
            event: Option<String>,
        }

        let encoded = EncodedPublicationDate::deserialize(deserializer)?;
        let decoded = Self::new(encoded.id, encoded.value, encoded.event).map_err(serde::de::Error::custom)?;
        if encoded.normalized != decoded.normalized {
            return Err(serde::de::Error::custom("publication date normalization does not match its source value"));
        }
        Ok(decoded)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationSubject {
    id: Option<String>,
    name: String,
    authority: Option<String>,
    code: Option<String>,
}

impl PublicationSubject {
    pub fn new(id: Option<String>, name: impl AsRef<str>, authority: Option<String>, code: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self {
            id: id.map(|value| bounded_publication_text(&value)).transpose()?,
            name: bounded_publication_text(name.as_ref())?,
            authority: authority.map(|value| bounded_publication_text(&value)).transpose()?,
            code: code.map(|value| bounded_publication_text(&value)).transpose()?,
        })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationSubject {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            name: String,
            authority: Option<String>,
            code: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.name, encoded.authority, encoded.code).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationCollection {
    name: String,
    collection_type: Option<String>,
    position: Option<String>,
}

impl PublicationCollection {
    pub fn new(name: impl AsRef<str>, collection_type: Option<String>, position: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self { name: bounded_publication_text(name.as_ref())?, collection_type: collection_type.map(|value| bounded_publication_text(&value)).transpose()?, position: position.map(|value| bounded_publication_text(&value)).transpose()? })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn collection_type(&self) -> Option<&str> {
        self.collection_type.as_deref()
    }

    pub fn position(&self) -> Option<&str> {
        self.position.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationCollection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            name: String,
            collection_type: Option<String>,
            position: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.name, encoded.collection_type, encoded.position).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationTitle {
    id: Option<String>,
    value: String,
    title_type: Option<String>,
    sort_as: Option<String>,
}

impl PublicationTitle {
    pub fn new(id: Option<String>, value: impl AsRef<str>, title_type: Option<String>, sort_as: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self {
            id: id.map(|value| bounded_publication_text(&value)).transpose()?,
            value: bounded_publication_text(value.as_ref())?,
            title_type: title_type.map(|value| bounded_publication_text(&value)).transpose()?,
            sort_as: sort_as.map(|value| bounded_publication_text(&value)).transpose()?,
        })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn title_type(&self) -> Option<&str> {
        self.title_type.as_deref()
    }

    pub fn sort_as(&self) -> Option<&str> {
        self.sort_as.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationTitle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            value: String,
            title_type: Option<String>,
            sort_as: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.value, encoded.title_type, encoded.sort_as).map_err(serde::de::Error::custom)
    }
}

/// Lossless storage for useful EPUB metadata we do not yet project into a
/// dedicated field. `refines` retains EPUB 3 relationships between values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationProperty {
    id: Option<String>,
    property: String,
    value: String,
    refines: Option<String>,
    scheme: Option<String>,
    language: Option<String>,
}

impl PublicationProperty {
    pub fn new(id: Option<String>, property: impl AsRef<str>, value: impl AsRef<str>, refines: Option<String>, scheme: Option<String>, language: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self {
            id: id.map(|value| bounded_publication_text(&value)).transpose()?,
            property: bounded_publication_text(property.as_ref())?,
            value: bounded_publication_text(value.as_ref())?,
            refines: refines.map(|value| bounded_publication_text(&value)).transpose()?,
            scheme: scheme.map(|value| bounded_publication_text(&value)).transpose()?,
            language: language.map(|value| bounded_publication_text(&value)).transpose()?,
        })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn property(&self) -> &str {
        &self.property
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn refines(&self) -> Option<&str> {
        self.refines.as_deref()
    }

    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationProperty {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            property: String,
            value: String,
            refines: Option<String>,
            scheme: Option<String>,
            language: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.property, encoded.value, encoded.refines, encoded.scheme, encoded.language).map_err(serde::de::Error::custom)
    }
}

/// Bibliographic metadata that accompanies the title, author credits, and
/// description already present in [`BookMetadataState`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PublicationMetadata {
    #[serde(default)]
    pub identifiers: Vec<PublicationIdentifier>,
    #[serde(default)]
    pub publishers: Vec<PublisherCredit>,
    #[serde(default)]
    pub languages: Vec<LanguageTag>,
    #[serde(default)]
    pub dates: Vec<PublicationDate>,
    #[serde(default)]
    pub contributors: Vec<PublicationContributor>,
    #[serde(default)]
    pub subjects: Vec<PublicationSubject>,
    #[serde(default)]
    pub collections: Vec<PublicationCollection>,
    #[serde(default)]
    pub alternate_titles: Vec<PublicationTitle>,
    #[serde(default)]
    pub rights: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub relations: Vec<String>,
    #[serde(default)]
    pub coverage: Vec<String>,
    #[serde(default)]
    pub properties: Vec<PublicationProperty>,
}

impl PublicationMetadata {
    pub fn validate(&self) -> Result<(), PublicationMetadataError> {
        valid_publication_metadata(self).then_some(()).ok_or(PublicationMetadataError)
    }
}

impl<'de> Deserialize<'de> for PublicationMetadata {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Default, Deserialize)]
        struct Encoded {
            #[serde(default)]
            identifiers: Vec<PublicationIdentifier>,
            #[serde(default)]
            publishers: Vec<PublisherCredit>,
            #[serde(default)]
            languages: Vec<LanguageTag>,
            #[serde(default)]
            dates: Vec<PublicationDate>,
            #[serde(default)]
            contributors: Vec<PublicationContributor>,
            #[serde(default)]
            subjects: Vec<PublicationSubject>,
            #[serde(default)]
            collections: Vec<PublicationCollection>,
            #[serde(default)]
            alternate_titles: Vec<PublicationTitle>,
            #[serde(default)]
            rights: Vec<String>,
            #[serde(default)]
            sources: Vec<String>,
            #[serde(default)]
            types: Vec<String>,
            #[serde(default)]
            formats: Vec<String>,
            #[serde(default)]
            relations: Vec<String>,
            #[serde(default)]
            coverage: Vec<String>,
            #[serde(default)]
            properties: Vec<PublicationProperty>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        let decoded = Self {
            identifiers: encoded.identifiers,
            publishers: encoded.publishers,
            languages: encoded.languages,
            dates: encoded.dates,
            contributors: encoded.contributors,
            subjects: encoded.subjects,
            collections: encoded.collections,
            alternate_titles: encoded.alternate_titles,
            rights: encoded.rights,
            sources: encoded.sources,
            types: encoded.types,
            formats: encoded.formats,
            relations: encoded.relations,
            coverage: encoded.coverage,
            properties: encoded.properties,
        };
        decoded.validate().map_err(serde::de::Error::custom)?;
        Ok(decoded)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicationMetadataError;

impl fmt::Display for PublicationMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("publication metadata must contain bounded valid text")
    }
}

impl std::error::Error for PublicationMetadataError {}

const MAX_PUBLICATION_TEXT_BYTES: usize = 4096;
const MAX_PUBLICATION_ITEMS_PER_KIND: usize = 256;
const MAX_PUBLICATION_ITEMS_TOTAL: usize = 2048;

fn bounded_publication_text(value: &str) -> Result<String, PublicationMetadataError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_PUBLICATION_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(PublicationMetadataError);
    }
    Ok(value.to_owned())
}

fn normalized_publication_text(value: &str) -> Result<String, PublicationMetadataError> {
    let value = bounded_publication_text(value)?;
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (normalized.len() <= MAX_PUBLICATION_TEXT_BYTES).then_some(normalized).ok_or(PublicationMetadataError)
}

fn canonical_identifier_value(scheme: &PublicationIdentifierScheme, value: &str) -> Option<String> {
    match scheme {
        PublicationIdentifierScheme::Isbn => canonical_isbn(value),
        PublicationIdentifierScheme::Doi => canonical_doi(value),
        PublicationIdentifierScheme::Uuid => {
            let lower = value.trim().to_ascii_lowercase();
            let value = lower.strip_prefix("urn:uuid:").unwrap_or(&lower);
            Uuid::parse_str(value).ok().map(|value| value.hyphenated().to_string())
        }
        _ => None,
    }
}

fn canonical_isbn(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix("urn:isbn:").or_else(|| value.strip_prefix("URN:ISBN:")).unwrap_or(value);
    let compact = value.chars().filter(|character| !matches!(character, '-' | ' ')).map(|character| character.to_ascii_uppercase()).collect::<String>();
    let bytes = compact.as_bytes();
    let valid = match bytes.len() {
        10 => {
            bytes[..9].iter().all(u8::is_ascii_digit)
                && (bytes[9].is_ascii_digit() || bytes[9] == b'X')
                && bytes.iter().enumerate().map(|(index, byte)| u32::from(if *byte == b'X' { 10 } else { *byte - b'0' }) * (10 - index as u32)).sum::<u32>() % 11 == 0
        }
        13 => bytes.iter().all(u8::is_ascii_digit) && bytes.iter().enumerate().map(|(index, byte)| u32::from(*byte - b'0') * if index % 2 == 0 { 1 } else { 3 }).sum::<u32>() % 10 == 0,
        _ => false,
    };
    valid.then_some(compact)
}

fn canonical_doi(value: &str) -> Option<String> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    let value = lower
        .strip_prefix("https://doi.org/")
        .or_else(|| lower.strip_prefix("http://doi.org/"))
        .or_else(|| lower.strip_prefix("https://dx.doi.org/"))
        .or_else(|| lower.strip_prefix("http://dx.doi.org/"))
        .or_else(|| lower.strip_prefix("doi:"))
        .unwrap_or(&lower);
    let valid = value.starts_with("10.")
        && value.split_once('/').is_some_and(|(registrant, suffix)| (4..=9).contains(&registrant[3..].len()) && registrant[3..].bytes().all(|byte| byte.is_ascii_digit()) && !suffix.is_empty())
        && !value.bytes().any(|byte| byte.is_ascii_whitespace());
    valid.then_some(value.to_owned())
}

fn parse_partial_date(value: &str) -> Option<PartialDate> {
    let parts = value.split('-').collect::<Vec<_>>();
    if !(1..=3).contains(&parts.len()) || parts[0].len() != 4 || !parts[0].bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year = parts[0].parse::<i32>().ok()?;
    let month = match parts.get(1) {
        Some(part) if part.len() == 2 => Some(part.parse::<u8>().ok()?),
        Some(_) => return None,
        None => None,
    };
    if month.is_some_and(|month| !(1..=12).contains(&month)) {
        return None;
    }
    let day = match parts.get(2) {
        Some(part) if part.len() == 2 => Some(part.parse::<u8>().ok()?),
        Some(_) => return None,
        None => None,
    };
    if let Some(day) = day {
        let month = month?;
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let maximum = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if !(1..=maximum).contains(&day) {
            return None;
        }
    }
    Some(PartialDate { year, month, day })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BookLifecycleState {
    Present { metadata: BookMetadataState, added_at: UnixMillis },
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlacementState {
    Present { book_id: BookId, content_hash: ContentHash },
    Vacant { removed_book_id: BookId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadingPositionState {
    pub location: String,
    pub progress: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookMetadataState {
    pub title: String,
    pub authors: Vec<AuthorCredit>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub publication: PublicationMetadata,
}

/// Identity of one canonical replicated state cell. It is derived from a
/// mutation body, never supplied independently on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    Directory,
    BookLifecycle,
    Placement,
    ReadingPosition,
    Annotation,
    Bookmark,
    BookLink,
    LastOpened,
    Metadata,
}

impl StateKind {
    pub const ALL: [Self; 9] = [Self::Directory, Self::BookLifecycle, Self::Placement, Self::ReadingPosition, Self::Annotation, Self::Bookmark, Self::BookLink, Self::LastOpened, Self::Metadata];

    pub const fn policy(self) -> StatePolicy {
        // This exhaustive declaration forces every new state domain to choose
        // conflict semantics instead of inheriting them accidentally.
        match self {
            Self::Directory | Self::BookLifecycle | Self::ReadingPosition | Self::Annotation | Self::Bookmark | Self::BookLink | Self::LastOpened | Self::Metadata => {
                StatePolicy { merge: MergePolicy::LastWriterWins { maximum_future_skew_ms: MAX_LWW_FUTURE_SKEW_MS }, replacement: ReplacementPolicy::Unconditional }
            }
            Self::Placement => StatePolicy { merge: MergePolicy::ServerOrderedConditional { maximum_future_skew_ms: MAX_LWW_FUTURE_SKEW_MS }, replacement: ReplacementPolicy::PlacementRemovalTargetsCurrentBook },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergePolicy {
    /// The greatest wall-clock timestamp wins. The remaining version fields
    /// exist only to make equal timestamps deterministic.
    LastWriterWins { maximum_future_skew_ms: u64 },
    /// Placement removals are compare-and-set transitions against the book
    /// currently occupying a physical path. They retain replica order and are
    /// serialized by the server because reducing them to one maximum value can
    /// change which book a removal observes.
    ServerOrderedConditional { maximum_future_skew_ms: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementPolicy {
    Unconditional,
    /// A removal identifies the book it observed at a path. It cannot remove a
    /// different book which subsequently took ownership of that path.
    PlacementRemovalTargetsCurrentBook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatePolicy {
    pub merge: MergePolicy,
    pub replacement: ReplacementPolicy,
}

impl StatePolicy {
    pub const fn maximum_accepted_timestamp(self, server_now_ms: u64) -> u64 {
        match self.merge {
            MergePolicy::LastWriterWins { maximum_future_skew_ms } | MergePolicy::ServerOrderedConditional { maximum_future_skew_ms } => server_now_ms.saturating_add(maximum_future_skew_ms),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StateKey {
    Directory(DirId),
    BookLifecycle(BookId),
    Placement { dir_id: DirId, file_name: FileName },
    ReadingPosition(BookId),
    Annotation(String),
    Bookmark(String),
    BookLink(String),
    LastOpened(BookId),
    Metadata(BookId),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MutationBody {
    Directory { dir_id: DirId, value: DirectoryState },
    BookLifecycle { book_id: BookId, value: BookLifecycleState },
    Placement { dir_id: DirId, file_name: FileName, value: PlacementState },
    ReadingPosition { book_id: BookId, value: ReadingPositionState },
    Annotation { annotation_id: String, value: AnnotationState },
    Bookmark { bookmark_id: String, value: BookmarkState },
    BookLink { link_id: String, value: BookLinkState },
    LastOpened { book_id: BookId, last_read: UnixMillis },
    Metadata { book_id: BookId, value: BookMetadataState },
}

/// A protocol-domain violation in a [`MutationBody`].
///
/// This validation is shared by producers and consumers. Transport, storage,
/// authorization, and dependency checks remain the responsibility of each
/// boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolValidationError {
    RootDirectoryMutation,
    DirectorySelfParent,
    InvalidBookTitle,
    InvalidReadingPosition,
    InvalidReadProgress,
    InvalidAnnotation,
    InvalidBookmark,
    InvalidBookLink,
    InvalidMetadataTitle,
    InvalidMetadataAuthor,
    InvalidMetadataDescription,
    InvalidPublicationMetadata,
}

impl fmt::Display for ProtocolValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RootDirectoryMutation => "the reserved root directory cannot be mutated",
            Self::DirectorySelfParent => "a directory cannot be its own parent",
            Self::InvalidBookTitle => "a book title must be bounded text without control characters",
            Self::InvalidReadingPosition => "a reading position must be a bounded EPUB CFI or PDF page",
            Self::InvalidReadProgress => "reading progress must be finite and between zero and one hundred",
            Self::InvalidAnnotation => "annotation state is invalid",
            Self::InvalidBookmark => "bookmark state is invalid",
            Self::InvalidBookLink => "book link state is invalid",
            Self::InvalidMetadataTitle => "a metadata title must be bounded text without control characters",
            Self::InvalidMetadataAuthor => "a metadata author must be bounded text without control characters",
            Self::InvalidMetadataDescription => "a metadata description must be bounded text",
            Self::InvalidPublicationMetadata => "publication metadata must be bounded and structurally valid",
        })
    }
}

impl std::error::Error for ProtocolValidationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequenceError;

impl fmt::Display for SequenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sequence numbers must be between one and the signed 64-bit database limit")
    }
}

impl std::error::Error for SequenceError {}

macro_rules! sequence_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, SequenceError> {
                if value > i64::MAX as u64 {
                    return Err(SequenceError);
                }
                NonZeroU64::new(value).map(Self).ok_or(SequenceError)
            }

            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.get().fmt(formatter)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = u64::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

sequence_type!(ReplicaSeq);
sequence_type!(LibraryRevision);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateMutation {
    /// Stable retry/idempotency key used for conditional outbox acknowledgement.
    pub mutation_id: MutationId,
    pub body: MutationBody,
    pub changed_at: UnixMillis,
    pub replica_seq: ReplicaSeq,
}

/// Reduces pending mutations produced by one replica without changing their
/// observable server-side meaning.
///
/// Ordinary state domains are unconditional registers and safely collapse to
/// their greatest local version. Placements are conditional transitions: an
/// add followed by a removal can produce a different result depending on the
/// server's current occupant, so every unacknowledged placement transition is
/// retained in replica sequence order.
pub fn reduce_same_replica_mutations<'a>(mutations: impl IntoIterator<Item = &'a StateMutation>) -> Vec<&'a StateMutation> {
    let mut registers = HashMap::<StateKey, &'a StateMutation>::new();
    let mut reduced = Vec::new();
    for mutation in mutations {
        if matches!(mutation.body, MutationBody::Placement { .. }) {
            reduced.push(mutation);
            continue;
        }
        let key = mutation.body.state_key();
        match registers.get(&key) {
            Some(current) if !same_replica_candidate_wins(current, mutation) => {}
            _ => {
                registers.insert(key, mutation);
            }
        }
    }
    reduced.extend(registers.into_values());
    reduced.sort_by_key(|mutation| (mutation.replica_seq, mutation.mutation_id));
    reduced
}

fn same_replica_candidate_wins(current: &StateMutation, candidate: &StateMutation) -> bool {
    candidate.body.may_replace(&current.body)
        && (candidate.changed_at, candidate.body.conflict_rank(), candidate.replica_seq, candidate.mutation_id) > (current.changed_at, current.body.conflict_rank(), current.replica_seq, current.mutation_id)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerMutation {
    pub mutation: StateMutation,
    pub replica_id: ClientId,
    pub revision: LibraryRevision,
}

impl Deref for ServerMutation {
    type Target = StateMutation;

    fn deref(&self) -> &Self::Target {
        &self.mutation
    }
}

/// Total conflict-resolution key shared by clients and the server.
///
/// `changed_at` supplies the user-facing last-writer-wins order.
/// `conflict_rank` makes destructive state win an exact timestamp tie, while
/// replica identity, sequence, and mutation identity make all remaining ties
/// deterministic regardless of delivery order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionKey {
    pub changed_at: UnixMillis,
    pub conflict_rank: u8,
    pub replica_id: ClientId,
    pub replica_seq: u64,
    pub mutation_id: MutationId,
}

impl VersionKey {
    pub fn new(changed_at: UnixMillis, conflict_rank: u8, replica_id: ClientId, replica_seq: u64, mutation_id: MutationId) -> Self {
        Self { changed_at, conflict_rank, replica_id, replica_seq, mutation_id }
    }
}

impl MutationBody {
    pub fn state_key(&self) -> StateKey {
        match self {
            Self::Directory { dir_id, .. } => StateKey::Directory(dir_id.clone()),
            Self::BookLifecycle { book_id, .. } => StateKey::BookLifecycle(*book_id),
            Self::Placement { dir_id, file_name, .. } => StateKey::Placement { dir_id: dir_id.clone(), file_name: file_name.clone() },
            Self::ReadingPosition { book_id, .. } => StateKey::ReadingPosition(*book_id),
            Self::Annotation { annotation_id, .. } => StateKey::Annotation(annotation_id.clone()),
            Self::Bookmark { bookmark_id, .. } => StateKey::Bookmark(bookmark_id.clone()),
            Self::BookLink { link_id, .. } => StateKey::BookLink(link_id.clone()),
            Self::LastOpened { book_id, .. } => StateKey::LastOpened(*book_id),
            Self::Metadata { book_id, .. } => StateKey::Metadata(*book_id),
        }
    }

    /// Stable storage discriminator. Unlike the former event type this names a
    /// canonical state domain; every mutation in a domain has one state key.
    pub const fn state_kind(&self) -> StateKind {
        match self {
            Self::Directory { .. } => StateKind::Directory,
            Self::BookLifecycle { .. } => StateKind::BookLifecycle,
            Self::Placement { .. } => StateKind::Placement,
            Self::ReadingPosition { .. } => StateKind::ReadingPosition,
            Self::Annotation { .. } => StateKind::Annotation,
            Self::Bookmark { .. } => StateKind::Bookmark,
            Self::BookLink { .. } => StateKind::BookLink,
            Self::LastOpened { .. } => StateKind::LastOpened,
            Self::Metadata { .. } => StateKind::Metadata,
        }
    }

    pub const fn policy(&self) -> StatePolicy {
        self.state_kind().policy()
    }

    /// Domain-specific precondition applied before the declared merge order.
    /// This is deliberately centralized with the policy declaration so every
    /// implementation agrees on whether two values describe the same state
    /// transition.
    pub fn may_replace(&self, current: &Self) -> bool {
        if self.state_key() != current.state_key() {
            return false;
        }
        match self.policy().replacement {
            ReplacementPolicy::Unconditional => true,
            ReplacementPolicy::PlacementRemovalTargetsCurrentBook => match (self, current) {
                (Self::Placement { value: PlacementState::Present { .. }, .. }, Self::Placement { .. }) => true,
                (Self::Placement { value: PlacementState::Vacant { removed_book_id }, .. }, Self::Placement { value: PlacementState::Present { book_id, .. }, .. }) => removed_book_id == book_id,
                (Self::Placement { value: PlacementState::Vacant { removed_book_id }, .. }, Self::Placement { value: PlacementState::Vacant { removed_book_id: current_book_id }, .. }) => removed_book_id == current_book_id,
                _ => false,
            },
        }
    }

    /// Rank used inside [`VersionKey`]. Tombstones in unconditional registers
    /// win an exact timestamp tie. Conditional placement transitions all use
    /// rank zero so a later sequence from one replica cannot lose to its own
    /// earlier removal merely because both occurred in the same millisecond.
    pub fn conflict_rank(&self) -> u8 {
        match self {
            Self::Directory { value: DirectoryState::Deleted { .. }, .. }
            | Self::BookLifecycle { value: BookLifecycleState::Deleted, .. }
            | Self::Annotation { value: AnnotationState { deleted: true, .. }, .. }
            | Self::Bookmark { value: BookmarkState { deleted: true, .. }, .. }
            | Self::BookLink { value: BookLinkState { deleted: true, .. }, .. } => 1,
            _ => 0,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        match self {
            Self::Directory { dir_id, .. } if dir_id.as_str() == "__root__" => Err(ProtocolValidationError::RootDirectoryMutation),
            Self::Directory { dir_id, value } if dir_id == value.parent_id() => Err(ProtocolValidationError::DirectorySelfParent),
            Self::BookLifecycle { value: BookLifecycleState::Present { metadata, .. }, .. } if !valid_metadata_text(&metadata.title) => Err(ProtocolValidationError::InvalidBookTitle),
            Self::BookLifecycle { value: BookLifecycleState::Present { metadata, .. }, .. } if !valid_metadata_authors(&metadata.authors) => Err(ProtocolValidationError::InvalidMetadataAuthor),
            Self::BookLifecycle { value: BookLifecycleState::Present { metadata, .. }, .. } if !valid_metadata_description(&metadata.description) => Err(ProtocolValidationError::InvalidMetadataDescription),
            Self::BookLifecycle { value: BookLifecycleState::Present { metadata, .. }, .. } if !valid_publication_metadata(&metadata.publication) => Err(ProtocolValidationError::InvalidPublicationMetadata),
            Self::ReadingPosition { value, .. } if !valid_reading_position(&value.location) => Err(ProtocolValidationError::InvalidReadingPosition),
            Self::ReadingPosition { value, .. } if !value.progress.is_finite() || !(0.0..=100.0).contains(&value.progress) => Err(ProtocolValidationError::InvalidReadProgress),
            Self::Annotation { annotation_id, value } if !valid_uuid(annotation_id) || !valid_annotation(value) => Err(ProtocolValidationError::InvalidAnnotation),
            Self::Bookmark { bookmark_id, value } if !valid_uuid(bookmark_id) || !valid_bookmark(value) => Err(ProtocolValidationError::InvalidBookmark),
            Self::BookLink { link_id, value } if !valid_uuid(link_id) || !valid_book_link(value) => Err(ProtocolValidationError::InvalidBookLink),
            Self::Metadata { value, .. } if !valid_metadata_text(&value.title) => Err(ProtocolValidationError::InvalidMetadataTitle),
            Self::Metadata { value, .. } if !valid_metadata_authors(&value.authors) => Err(ProtocolValidationError::InvalidMetadataAuthor),
            Self::Metadata { value, .. } if !valid_metadata_description(&value.description) => Err(ProtocolValidationError::InvalidMetadataDescription),
            Self::Metadata { value, .. } if !valid_publication_metadata(&value.publication) => Err(ProtocolValidationError::InvalidPublicationMetadata),
            _ => Ok(()),
        }
    }
}

fn valid_metadata_text(value: &str) -> bool {
    value.len() <= 4096 && !value.chars().any(char::is_control)
}

fn valid_metadata_authors(authors: &[AuthorCredit]) -> bool {
    authors.len() <= 64 && authors.iter().map(AuthorCredit::encoded_text_len).sum::<usize>() <= 16 * 1024
}

fn valid_metadata_description(value: &str) -> bool {
    value.len() <= 65_536 && !value.chars().any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn valid_publication_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_PUBLICATION_TEXT_BYTES && !value.chars().any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn valid_publication_metadata(value: &PublicationMetadata) -> bool {
    let counts = [
        value.identifiers.len(),
        value.publishers.len(),
        value.languages.len(),
        value.dates.len(),
        value.contributors.len(),
        value.subjects.len(),
        value.collections.len(),
        value.alternate_titles.len(),
        value.rights.len(),
        value.sources.len(),
        value.types.len(),
        value.formats.len(),
        value.relations.len(),
        value.coverage.len(),
        value.properties.len(),
    ];
    counts.iter().all(|count| *count <= MAX_PUBLICATION_ITEMS_PER_KIND)
        && counts.iter().sum::<usize>() <= MAX_PUBLICATION_ITEMS_TOTAL
        && value.identifiers.iter().all(|identifier| {
            valid_publication_value(identifier.value())
                && identifier.identifier_types().iter().all(|kind| valid_publication_value(kind.value()) && kind.scheme().is_none_or(valid_publication_value))
                && !matches!(identifier.scheme(), PublicationIdentifierScheme::Other(other) if !valid_publication_value(other))
        })
        && value.dates.iter().all(|date| date.id().is_none_or(valid_publication_value) && valid_publication_value(date.value()) && date.event().is_none_or(valid_publication_value))
        && value.contributors.iter().all(|contributor| valid_publication_value(contributor.name()) && contributor.sort_name().is_none_or(valid_publication_value))
        && value
            .subjects
            .iter()
            .all(|subject| subject.id().is_none_or(valid_publication_value) && valid_publication_value(subject.name()) && subject.authority().is_none_or(valid_publication_value) && subject.code().is_none_or(valid_publication_value))
        && value.collections.iter().all(|collection| valid_publication_value(collection.name()) && collection.collection_type().is_none_or(valid_publication_value) && collection.position().is_none_or(valid_publication_value))
        && value
            .alternate_titles
            .iter()
            .all(|title| title.id().is_none_or(valid_publication_value) && valid_publication_value(title.value()) && title.title_type().is_none_or(valid_publication_value) && title.sort_as().is_none_or(valid_publication_value))
        && [&value.rights, &value.sources, &value.types, &value.formats, &value.relations, &value.coverage].into_iter().flatten().all(|text| valid_publication_value(text))
        && value.properties.iter().all(|property| {
            property.id().is_none_or(valid_publication_value)
                && valid_publication_value(property.property())
                && valid_publication_value(property.value())
                && property.refines().is_none_or(valid_publication_value)
                && property.scheme().is_none_or(valid_publication_value)
                && property.language().is_none_or(valid_publication_value)
        })
}

fn valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

fn valid_reader_text(value: &str, maximum: usize) -> bool {
    value.len() <= maximum && !value.chars().any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn valid_annotation(value: &AnnotationState) -> bool {
    value.anchor.is_valid()
        && valid_reader_text(&value.exact_text, 64 * 1024)
        && value.prefix.as_deref().is_none_or(|text| valid_reader_text(text, 4096))
        && value.suffix.as_deref().is_none_or(|text| valid_reader_text(text, 4096))
        && valid_reader_text(&value.note, 64 * 1024)
        && value.color.len() <= 64
        && !value.color.is_empty()
        && value.modified_at >= value.created_at
}

fn valid_bookmark(value: &BookmarkState) -> bool {
    valid_epub_cfi(&value.cfi) && valid_reader_text(&value.label, 4096) && value.modified_at >= value.created_at
}

fn valid_book_link(value: &BookLinkState) -> bool {
    value.source_book_id != value.target_book_id
        && !value.relation.trim().is_empty()
        && value.relation.trim() == value.relation
        && valid_reader_text(&value.relation, 255)
        && valid_reader_text(&value.note, 64 * 1024)
        && value.modified_at >= value.created_at
}

impl ServerMutation {
    pub fn version_key(&self) -> VersionKey {
        VersionKey::new(self.changed_at, self.body.conflict_rank(), self.replica_id.clone(), self.replica_seq.get(), self.mutation_id)
    }

    pub fn wins_over(&self, current: &Self) -> bool {
        self.body.may_replace(&current.body) && self.version_key() > current.version_key()
    }
}

impl StateMutation {
    /// Applies the admission rules declared by this state domain. Servers call
    /// this with their current Unix time. Offline clients only validate the
    /// body because they cannot impose the server's clock policy.
    pub fn rejection_at(&self, server_now_ms: u64) -> Option<MutationRejectionReason> {
        if self.body.validate().is_err() {
            return Some(MutationRejectionReason::InvalidPayload);
        }
        if self.changed_at.get() > self.body.policy().maximum_accepted_timestamp(server_now_ms) {
            return Some(MutationRejectionReason::TimestampTooFarFuture);
        }
        None
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushMutationsRequest {
    pub library_id: LibraryId,
    pub replica_id: ClientId,
    pub mutations: Vec<StateMutation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushMutationsResponse {
    #[serde(default)]
    pub accepted: Vec<MutationId>,
    #[serde(default)]
    pub rejected: Vec<MutationRejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationRejection {
    pub mutation_id: MutationId,
    pub reason: MutationRejectionReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationRejectionReason {
    TimestampTooFarFuture,
    InvalidPayload,
    MissingDependency,
}

impl MutationRejectionReason {
    /// Whether the rejection is temporary, so the change should be retried later
    /// rather than dropped from the outbox. Clock-skew rejections are transient;
    /// other malformed changes are rejected during deserialization.
    pub fn is_transient(self) -> bool {
        matches!(self, MutationRejectionReason::TimestampTooFarFuture | MutationRejectionReason::MissingDependency)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullStateResponse {
    pub mutations: Vec<ServerMutation>,
    pub next_cursor: Option<LibraryRevision>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullStateQuery {
    pub library_id: LibraryId,
    pub replica_id: ClientId,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_library_cursor")]
    pub cursor: Option<LibraryRevision>,
}

/// One canonical bidirectional state exchange.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncExchangeRequest {
    pub library_id: LibraryId,
    pub replica_id: ClientId,
    #[serde(default)]
    pub mutations: Vec<StateMutation>,
    #[serde(default, deserialize_with = "deserialize_library_cursor")]
    pub cursor: Option<LibraryRevision>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncExchangeResponse {
    pub push: PushMutationsResponse,
    pub pull: PullStateResponse,
}

fn deserialize_library_cursor<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<LibraryRevision>, D::Error> {
    match Option::<u64>::deserialize(deserializer)? {
        None | Some(0) => Ok(None),
        Some(value) => LibraryRevision::new(value).map(Some).map_err(serde::de::Error::custom),
    }
}

#[cfg(test)]
mod canonical_state_tests {
    use super::*;

    fn dir(value: &str) -> DirId {
        DirId::parse(value).unwrap()
    }

    fn file(value: &str) -> FileName {
        FileName::parse(value).unwrap()
    }

    #[test]
    fn every_mutation_derives_its_canonical_key() {
        let body = MutationBody::Placement { dir_id: dir("shelf"), file_name: file("book.epub"), value: PlacementState::Present { book_id: BookId::new(7), content_hash: ContentHash::parse(&"a".repeat(64)).unwrap() } };
        assert_eq!(body.state_key(), StateKey::Placement { dir_id: dir("shelf"), file_name: file("book.epub") });
        assert_eq!(body.state_kind(), StateKind::Placement);
    }

    #[test]
    fn only_unconditional_tombstones_use_destructive_tie_priority() {
        assert_eq!(MutationBody::BookLifecycle { book_id: BookId::new(1), value: BookLifecycleState::Deleted }.conflict_rank(), 1);
        assert_eq!(MutationBody::Placement { dir_id: dir("shelf"), file_name: file("book.epub"), value: PlacementState::Vacant { removed_book_id: BookId::new(1) } }.conflict_rank(), 0,);
    }

    #[test]
    fn wire_round_trips_canonical_state_variants_and_explicit_cursor() {
        let positional = MutationBody::LastOpened { book_id: BookId::new(42), last_read: UnixMillis::new(5).unwrap() };
        assert_eq!(wire::encode(&positional).unwrap(), b"BKHM\x05\x07\x2a\x05");
        assert_eq!(wire::decode::<MutationBody>(b"BKHM\x05\x07\x2a\x05", 64).unwrap(), positional);

        let mutation = StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: "epubcfi(/6/4)".into(), progress: 42.0 } },
            changed_at: UnixMillis::new(1).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        let encoded = wire::encode(&mutation).unwrap();
        let decoded: StateMutation = wire::decode(&encoded, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
        assert_eq!(decoded, mutation);
        let response = PullStateResponse { mutations: Vec::new(), next_cursor: LibraryRevision::new(9).ok(), has_more: false };
        let encoded = wire::encode(&response).unwrap();
        let decoded: PullStateResponse = wire::decode(&encoded, wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
        assert_eq!(decoded.next_cursor.unwrap().get(), 9);
    }

    #[test]
    fn binary_identifiers_use_fixed_bytes_while_json_keeps_canonical_strings() {
        let mutation_id = MutationId::parse("00112233-4455-6677-8899-aabbccddeeff").unwrap();
        let encoded = wire::encode(&mutation_id).unwrap();
        assert_eq!(&encoded[..5], b"BKHM\x05");
        assert_eq!(&encoded[5..], &[0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        assert_eq!(wire::decode::<MutationId>(&encoded, 64).unwrap(), mutation_id);

        let hash = ContentHash::parse(&"a".repeat(64)).unwrap();
        let encoded = wire::encode(&hash).unwrap();
        assert_eq!(encoded.len(), 5 + 32);
        assert!(encoded[5..].iter().all(|byte| *byte == 0xaa));
        assert_eq!(wire::decode::<ContentHash>(&encoded, 64).unwrap(), hash);

        assert_eq!(serde_json::to_string(&mutation_id).unwrap(), "\"00112233-4455-6677-8899-aabbccddeeff\"");
        assert_eq!(serde_json::to_string(&hash).unwrap(), format!("\"{}\"", "a".repeat(64)));
    }

    #[test]
    fn wire_preserves_ordered_author_credits() {
        let publication = PublicationMetadata {
            identifiers: vec![PublicationIdentifier::new("9781234567897", PublicationIdentifierScheme::Isbn, vec![PublicationIdentifierType::new("15", Some("onix:codelist5".to_owned())).unwrap()]).unwrap()],
            publishers: vec![PublisherCredit::new("North Wind Press").unwrap()],
            languages: vec![LanguageTag::parse("sv-SE").unwrap()],
            dates: vec![PublicationDate::new(Some("publication-date".to_owned()), "2026-08-04", Some("publication".to_owned())).unwrap()],
            contributors: vec![PublicationContributor::new("Example Translator", Some("Translator, Example".to_owned()), MarcRelatorCode::parse("trl").unwrap()).unwrap()],
            subjects: vec![PublicationSubject::new(Some("subject-1".to_owned()), "Distributed systems", Some("BISAC".to_owned()), Some("COM046000".to_owned())).unwrap()],
            collections: vec![PublicationCollection::new("Bokheim Studies", Some("series".to_owned()), Some("2".to_owned())).unwrap()],
            alternate_titles: vec![PublicationTitle::new(Some("subtitle".to_owned()), "An offline-first study", Some("subtitle".to_owned()), None).unwrap()],
            rights: vec!["Copyright example".to_owned()],
            properties: vec![PublicationProperty::new(Some("meta-1".to_owned()), "dcterms:modified", "2026-08-04T12:00:00Z", None, None, None).unwrap()],
            ..PublicationMetadata::default()
        };
        let mutation = StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::Metadata {
                book_id: BookId::new(1),
                value: BookMetadataState {
                    title: "A collaboration".to_owned(),
                    authors: vec![AuthorCredit::new("Le Guin, Ursula K.", Some("Le Guin, Ursula K.".to_owned())).unwrap(), AuthorCredit::from_name("Jane Doe").unwrap()],
                    description: String::new(),
                    publication,
                },
            },
            changed_at: UnixMillis::new(1).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };

        let encoded = wire::encode(&mutation).unwrap();
        let decoded: StateMutation = wire::decode(&encoded, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
        assert_eq!(decoded, mutation);
    }

    #[test]
    fn publication_agents_keep_stable_ids_across_wire_round_trips() {
        let publisher = PublisherCredit::new("North Wind Press").unwrap();
        let contributor = PublicationContributor::new("Example Translator", Some("Translator, Example".to_owned()), MarcRelatorCode::parse("trl").unwrap()).unwrap();
        let publication = PublicationMetadata { publishers: vec![publisher.clone()], contributors: vec![contributor.clone()], ..PublicationMetadata::default() };

        let encoded = wire::encode(&publication).unwrap();
        let decoded = wire::decode::<PublicationMetadata>(&encoded, 4096).unwrap();
        assert_eq!(decoded.publishers[0].agent_id(), publisher.agent_id());
        assert_eq!(decoded.contributors[0].agent_id(), contributor.agent_id());
    }

    #[test]
    fn identifiers_only_expose_verified_canonical_values() {
        let isbn = PublicationIdentifier::new("urn:isbn:978-1-23456-789-7", PublicationIdentifierScheme::Isbn, Vec::new()).unwrap();
        assert_eq!(isbn.canonical_value(), Some("9781234567897"));
        assert_eq!(isbn.lookup_value(), "9781234567897");

        let invalid_isbn = PublicationIdentifier::new("978-1-23456-789-8", PublicationIdentifierScheme::Isbn, Vec::new()).unwrap();
        assert_eq!(invalid_isbn.canonical_value(), None);
        assert_eq!(invalid_isbn.lookup_value(), "978-1-23456-789-8");

        let doi = PublicationIdentifier::new("HTTPS://DOI.ORG/10.1000/Example", PublicationIdentifierScheme::Doi, Vec::new()).unwrap();
        assert_eq!(doi.canonical_value(), Some("10.1000/example"));

        let invalid_doi = PublicationIdentifier::new("10./not-a-doi", PublicationIdentifierScheme::Doi, Vec::new()).unwrap();
        assert_eq!(invalid_doi.canonical_value(), None);
    }

    #[test]
    fn publication_dates_normalize_only_real_calendar_dates() {
        let leap_day = PublicationDate::new(None, "2024-02-29", None).unwrap();
        let normalized = leap_day.normalized().unwrap();
        assert_eq!((normalized.year(), normalized.month(), normalized.day()), (2024, Some(2), Some(29)));

        let invalid_leap_day = PublicationDate::new(None, "2023-02-29", None).unwrap();
        assert_eq!(invalid_leap_day.normalized(), None);

        let year_only = PublicationDate::new(None, "1965", None).unwrap();
        assert_eq!(year_only.normalized().unwrap().year(), 1965);
        assert_eq!(year_only.normalized().unwrap().month(), None);
    }

    #[test]
    fn publication_metadata_deserialization_enforces_leaf_and_collection_bounds() {
        let mut invalid_text = PublicationMetadata::default();
        invalid_text.rights.push("x".repeat(MAX_PUBLICATION_TEXT_BYTES + 1));
        let encoded = wire::encode(&invalid_text).unwrap();
        assert!(wire::decode::<PublicationMetadata>(&encoded, 16 * 1024).is_err());

        let mut too_many = PublicationMetadata::default();
        too_many.rights = (0..=MAX_PUBLICATION_ITEMS_PER_KIND).map(|index| format!("right {index}")).collect();
        let encoded = wire::encode(&too_many).unwrap();
        assert!(wire::decode::<PublicationMetadata>(&encoded, 64 * 1024).is_err());
    }

    #[test]
    fn derived_identifier_values_cannot_be_forged_on_the_wire() {
        #[derive(Serialize)]
        struct UncheckedIdentifier {
            value: String,
            canonical_value: Option<String>,
            scheme: PublicationIdentifierScheme,
            identifier_types: Vec<PublicationIdentifierType>,
        }

        let encoded = wire::encode(&UncheckedIdentifier { value: "9781234567897".to_owned(), canonical_value: Some("9780000000000".to_owned()), scheme: PublicationIdentifierScheme::Isbn, identifier_types: Vec::new() }).unwrap();
        assert!(wire::decode::<PublicationIdentifier>(&encoded, 4096).is_err());
    }

    #[test]
    fn author_credit_deserialization_cannot_bypass_validation() {
        #[derive(Serialize)]
        struct UncheckedAuthorCredit {
            author_id: String,
            name: String,
            sort_name: Option<String>,
        }

        let empty_name = wire::encode(&UncheckedAuthorCredit { author_id: AuthorId::new().to_string(), name: "  ".to_owned(), sort_name: None }).unwrap();
        assert!(wire::decode::<AuthorCredit>(&empty_name, 1024).is_err());
        let invalid_sort_name = wire::encode(&UncheckedAuthorCredit { author_id: AuthorId::new().to_string(), name: "Valid author".to_owned(), sort_name: Some("bad\nsort".to_owned()) }).unwrap();
        assert!(wire::decode::<AuthorCredit>(&invalid_sort_name, 1024).is_err());
        let nil_id = wire::encode(&UncheckedAuthorCredit { author_id: Uuid::nil().to_string(), name: "Valid author".to_owned(), sort_name: None }).unwrap();
        assert!(wire::decode::<AuthorCredit>(&nil_id, 1024).is_err());
        assert_eq!(AuthorId::from_migration_seed(b"same legacy credit"), AuthorId::from_migration_seed(b"same legacy credit"));
    }

    #[test]
    fn protocol_validation_bounds_the_complete_author_list() {
        let metadata = |authors| MutationBody::Metadata { book_id: BookId::new(1), value: BookMetadataState { title: "Book".to_owned(), authors, description: String::new(), publication: PublicationMetadata::default() } };

        assert!(AuthorCredit::from_name("").is_err());
        assert!(AuthorCredit::from_name("a".repeat(4097)).is_err());
        assert_eq!(metadata((0..65).map(|index| AuthorCredit::from_name(format!("Author {index}")).unwrap()).collect()).validate(), Err(ProtocolValidationError::InvalidMetadataAuthor));
    }

    #[test]
    fn unix_milliseconds_round_trip_with_storage_bounds() {
        let timestamp = UnixMillis::new(1_700_000_000_123).unwrap();
        let encoded = wire::encode(&timestamp).unwrap();
        assert_eq!(wire::decode::<UnixMillis>(&encoded, 1024).unwrap(), timestamp);
        assert!(UnixMillis::new(i64::MAX as u64 + 1).is_err());
    }

    #[test]
    fn protocol_validation_rejects_invalid_canonical_values() {
        let invalid = MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: "not-a-location".into(), progress: 0.0 } };
        assert_eq!(invalid.validate(), Err(ProtocolValidationError::InvalidReadingPosition));
    }

    #[test]
    fn every_state_domain_declares_its_merge_semantics() {
        for kind in StateKind::ALL {
            let expected = if kind == StateKind::Placement { MergePolicy::ServerOrderedConditional { maximum_future_skew_ms: MAX_LWW_FUTURE_SKEW_MS } } else { MergePolicy::LastWriterWins { maximum_future_skew_ms: MAX_LWW_FUTURE_SKEW_MS } };
            assert_eq!(kind.policy().merge, expected);
        }
    }

    #[test]
    fn unconditional_register_reduction_is_permutation_invariant() {
        let mutation = |sequence, changed_at, location: &str| StateMutation {
            mutation_id: MutationId::parse(&format!("00000000-0000-4000-8000-{sequence:012}")).unwrap(),
            body: MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: location.to_owned(), progress: changed_at as f32 } },
            changed_at: UnixMillis::new(changed_at).unwrap(),
            replica_seq: ReplicaSeq::new(sequence).unwrap(),
        };
        let mutations = [mutation(1, 10, "epubcfi(/6/2)"), mutation(2, 30, "epubcfi(/6/4)"), mutation(3, 20, "epubcfi(/6/6)")];
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let input = order.map(|index| &mutations[index]);
            let reduced = reduce_same_replica_mutations(input);
            assert_eq!(reduced.len(), 1);
            assert_eq!(reduced[0].mutation_id, mutations[1].mutation_id);
        }
    }

    #[test]
    fn conditional_placement_reduction_preserves_every_transition() {
        let path = |sequence, value| StateMutation {
            mutation_id: MutationId::parse(&format!("00000000-0000-4000-8000-{sequence:012}")).unwrap(),
            body: MutationBody::Placement { dir_id: dir("shelf"), file_name: file("book.epub"), value },
            changed_at: UnixMillis::new(sequence * 10).unwrap(),
            replica_seq: ReplicaSeq::new(sequence).unwrap(),
        };
        let mutations = [
            path(1, PlacementState::Present { book_id: BookId::new(1), content_hash: ContentHash::parse(&"a".repeat(64)).unwrap() }),
            path(2, PlacementState::Present { book_id: BookId::new(2), content_hash: ContentHash::parse(&"b".repeat(64)).unwrap() }),
            path(3, PlacementState::Vacant { removed_book_id: BookId::new(2) }),
        ];
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let input = order.map(|index| &mutations[index]);
            let reduced = reduce_same_replica_mutations(input);
            assert_eq!(reduced.iter().map(|mutation| mutation.replica_seq.get()).collect::<Vec<_>>(), vec![1, 2, 3]);
        }
    }

    #[test]
    fn placement_removal_only_targets_the_book_it_observed() {
        let path = |value| MutationBody::Placement { dir_id: dir("shelf"), file_name: file("book.epub"), value };
        let current = path(PlacementState::Present { book_id: BookId::new(2), content_hash: ContentHash::parse(&"b".repeat(64)).unwrap() });
        let stale_removal = path(PlacementState::Vacant { removed_book_id: BookId::new(1) });
        let matching_removal = path(PlacementState::Vacant { removed_book_id: BookId::new(2) });

        assert_eq!(StateKind::Placement.policy().replacement, ReplacementPolicy::PlacementRemovalTargetsCurrentBook);
        assert!(!stale_removal.may_replace(&current));
        assert!(matching_removal.may_replace(&current));
    }

    #[test]
    fn same_millisecond_placement_sequence_can_claim_after_removal() {
        let replica = ClientId::parse("scanner").unwrap();
        let removal = VersionKey::new(UnixMillis::new(10).unwrap(), 0, replica.clone(), 1, MutationId::new());
        let later_claim = VersionKey::new(UnixMillis::new(10).unwrap(), 0, replica, 2, MutationId::new());
        assert!(later_claim > removal);
    }

    #[test]
    fn server_rejects_lww_timestamps_beyond_the_clock_skew_allowance() {
        let server_now = 1_000_000;
        let mutation = |changed_at| StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: "epubcfi(/6/4)".into(), progress: 42.0 } },
            changed_at,
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };

        assert_eq!(mutation(UnixMillis::new(server_now + MAX_LWW_FUTURE_SKEW_MS).unwrap()).rejection_at(server_now), None);
        assert_eq!(mutation(UnixMillis::new(server_now + MAX_LWW_FUTURE_SKEW_MS + 1).unwrap()).rejection_at(server_now), Some(MutationRejectionReason::TimestampTooFarFuture),);
    }

    #[test]
    fn lww_order_uses_wall_clock_before_deterministic_tie_breakers() {
        let replica = ClientId::parse("reader").unwrap();
        let older = VersionKey::new(UnixMillis::new(10).unwrap(), 0, replica.clone(), 2, MutationId::new());
        let newer = VersionKey::new(UnixMillis::new(11).unwrap(), 0, replica, 1, MutationId::new());
        assert!(newer > older);
    }
}
