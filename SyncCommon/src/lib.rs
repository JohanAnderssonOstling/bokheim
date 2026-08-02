use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::fmt;
use std::num::NonZeroU64;
use std::ops::Deref;
use std::str::FromStr;
use uuid::Uuid;

pub mod api;

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
        serializer.serialize_str(&self.canonical())
    }
}

impl<'de> Deserialize<'de> for BookId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
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
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
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

domain_text_type!(ActorId, "actor id");
domain_text_type!(ClientId, "client id");
domain_text_type!(LibraryId, "library id");
domain_text_type!(DirId, "directory id");

/// Globally unique identity of one logical mutation. Unlike an actor-local
/// sequence this remains unique when a library database is copied and both
/// copies subsequently produce changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(Uuid);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventIdError;

impl fmt::Display for EventIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("event id must be a non-nil UUID")
    }
}

impl std::error::Error for EventIdError {}

impl EventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, EventIdError> {
        let value = Uuid::parse_str(value).map_err(|_| EventIdError)?;
        if value.is_nil() {
            Err(EventIdError)
        } else {
            Ok(Self(value))
        }
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for EventId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EventId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
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

/// Event types that collapse to one row per `(library_id, entity_id, event_type)`
/// under LWW semantics. All other event types are append-only and deduped by
/// `(library_id, event_id)`.
///
/// The Postgres partial indexes `idx_library_events_lww` / `idx_library_events_dedup`
/// in the latest server migration hardcode the same list — keep them in sync.
pub const SINGLETON_EVENT_TYPES: &[&str] = &["dir_state", "reading_position_updated", "book_opened", "book_metadata_updated"];

/// Renders `SINGLETON_EVENT_TYPES` as a parenthesised SQL list, e.g.
/// `('reading_position_updated', 'book_opened')`. Use to interpolate into
/// `WHERE event_type IN ...` / `NOT IN ...` so the SQL matches the constant.
pub fn singleton_event_types_sql_list() -> String {
    let quoted: Vec<String> = SINGLETON_EVENT_TYPES.iter().map(|s| format!("'{}'", s)).collect();
    format!("({})", quoted.join(", "))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DirectoryState {
    Present { parent_id: DirId, name: FileName },
    Deleted { parent_id: DirId, name: FileName },
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncOp {
    DirState { dir_id: DirId, directory: DirectoryState },
    BookAdded { book_id: BookId, title: String, added_at: i64 },
    BookDeleted { book_id: BookId },
    BookDirAdded { book_id: BookId, dir_id: DirId, file_name: FileName, content_hash: ContentHash },
    BookDirRemoved { book_id: BookId, dir_id: DirId, file_name: FileName },
    ReadingPositionUpdated { book_id: BookId, cfi: String },
    BookOpened { book_id: BookId, last_read: i64 },
    BookMetadataUpdated { book_id: BookId, title: String, author: String },
}

/// A protocol-domain violation in a [`SyncOp`].
///
/// This validation is shared by producers and consumers. Transport, storage,
/// authorization, and dependency checks remain the responsibility of each
/// boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolValidationError {
    RootDirectoryMutation,
    DirectorySelfParent,
    NegativeBookAddedTimestamp,
    InvalidBookTitle,
    InvalidReadingPosition,
    NegativeLastReadTimestamp,
    InvalidMetadataTitle,
    InvalidMetadataAuthor,
}

impl fmt::Display for ProtocolValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RootDirectoryMutation => "the reserved root directory cannot be mutated",
            Self::DirectorySelfParent => "a directory cannot be its own parent",
            Self::NegativeBookAddedTimestamp => "a book-added timestamp cannot be negative",
            Self::InvalidBookTitle => "a book title must be bounded text without control characters",
            Self::InvalidReadingPosition => "a reading position must be a bounded EPUB CFI or PDF page",
            Self::NegativeLastReadTimestamp => "a last-read timestamp cannot be negative",
            Self::InvalidMetadataTitle => "a metadata title must be bounded text without control characters",
            Self::InvalidMetadataAuthor => "a metadata author must be bounded text without control characters",
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

sequence_type!(LocalSeq);
sequence_type!(ServerSeq);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalChange {
    /// Stable retry/idempotency key. It is mandatory on the wire, and every
    /// newly persisted local event receives one before its first send.
    pub event_id: EventId,
    pub op: SyncOp,
    pub changed_at: u64,
    #[serde(default)]
    pub hlc: Hlc,
    pub local_seq: LocalSeq,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerChange {
    #[serde(flatten)]
    pub change: LocalChange,
    pub actor_id: ActorId,
    pub server_seq: ServerSeq,
}

impl Deref for ServerChange {
    type Target = LocalChange;

    fn deref(&self) -> &Self::Target {
        &self.change
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Hlc {
    pub pt: u64,
    pub l: u64,
}

/// Total conflict-resolution key shared by clients and the server.
///
/// HLC preserves causal/wall-clock ordering. `conflict_rank` makes destructive
/// state win an otherwise exact tie, while actor id and actor-local sequence
/// make concurrent values totally ordered regardless of delivery order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionKey {
    pub hlc_pt: u64,
    pub hlc_l: u64,
    pub conflict_rank: u8,
    pub actor_id: ActorId,
    pub actor_seq: u64,
    pub event_id: EventId,
}

impl VersionKey {
    pub fn new(hlc: Hlc, conflict_rank: u8, actor_id: ActorId, actor_seq: u64, event_id: EventId) -> Self {
        Self { hlc_pt: hlc.pt, hlc_l: hlc.l, conflict_rank, actor_id, actor_seq, event_id }
    }
}

impl Hlc {
    pub fn tick_recv(prev: Hlc, remote: Hlc, wall_now: u64) -> Hlc {
        let pt = prev.pt.max(remote.pt).max(wall_now);
        let l = match (pt == prev.pt, pt == remote.pt) {
            (true, true) => prev.l.max(remote.l).saturating_add(1),
            (true, false) => prev.l.saturating_add(1),
            (false, true) => remote.l.saturating_add(1),
            (false, false) => 0,
        };
        Hlc { pt, l }
    }
}

impl SyncOp {
    /// Whether a persisted event type is understood by this protocol build.
    /// Includes accepted legacy aliases that are no longer emitted.
    pub fn is_known_event_type(event_type: &str) -> bool {
        matches!(event_type, "dir_state" | "book_added" | "book_deleted" | "book_dir_added" | "book_dir_removed" | "book_dir_deleted" | "reading_position_updated" | "book_opened" | "book_metadata_updated")
    }

    pub fn event_type(&self) -> &'static str {
        match self {
            SyncOp::DirState { .. } => "dir_state",
            SyncOp::BookAdded { .. } => "book_added",
            SyncOp::BookDeleted { .. } => "book_deleted",
            SyncOp::BookDirAdded { .. } => "book_dir_added",
            SyncOp::BookDirRemoved { .. } => "book_dir_removed",
            SyncOp::ReadingPositionUpdated { .. } => "reading_position_updated",
            SyncOp::BookOpened { .. } => "book_opened",
            SyncOp::BookMetadataUpdated { .. } => "book_metadata_updated",
        }
    }

    pub fn entity_id(&self) -> Cow<'_, str> {
        match self {
            SyncOp::DirState { dir_id, .. } => Cow::Borrowed(dir_id.as_str()),
            SyncOp::BookAdded { book_id, .. }
            | SyncOp::BookDeleted { book_id }
            | SyncOp::BookDirAdded { book_id, .. }
            | SyncOp::BookDirRemoved { book_id, .. }
            | SyncOp::ReadingPositionUpdated { book_id, .. }
            | SyncOp::BookOpened { book_id, .. }
            | SyncOp::BookMetadataUpdated { book_id, .. } => Cow::Owned(book_id.canonical()),
        }
    }

    pub fn is_singleton(&self) -> bool {
        SINGLETON_EVENT_TYPES.contains(&self.event_type())
    }

    /// Rank used inside [`VersionKey`]. A tombstone wins an exact HLC tie, then
    /// actor identity and actor-local sequence break ties within the same rank.
    pub fn conflict_rank(&self) -> u8 {
        match self {
            SyncOp::DirState { directory: DirectoryState::Deleted { .. }, .. } | SyncOp::BookDeleted { .. } | SyncOp::BookDirRemoved { .. } => 1,
            _ => 0,
        }
    }

    pub fn payload(&self) -> Value {
        match self {
            SyncOp::DirState { directory, .. } => json!({ "parent_id": directory.parent_id(), "name": directory.name(), "deleted": if directory.is_deleted() { 1 } else { 0 } }),
            SyncOp::BookAdded { title, added_at, .. } => json!({ "title": title, "added_at": added_at }),
            SyncOp::BookDeleted { .. } => json!({}),
            SyncOp::BookDirAdded { dir_id, file_name, content_hash, .. } => json!({
                "dir_id": dir_id,
                "file_name": file_name,
                "content_hash": content_hash,
            }),
            SyncOp::BookDirRemoved { dir_id, file_name, .. } => json!({
                "dir_id": dir_id,
                "file_name": file_name,
            }),
            SyncOp::ReadingPositionUpdated { cfi, .. } => json!({ "cfi": cfi }),
            SyncOp::BookOpened { last_read, .. } => json!({ "last_read": last_read }),
            SyncOp::BookMetadataUpdated { title, author, .. } => json!({ "title": title, "author": author }),
        }
    }

    /// Reconstructs and validates an operation from its normalized storage
    /// columns. `None` means either decoding or protocol-domain validation
    /// failed.
    pub fn from_event_parts(entity_type: &str, entity_id: String, payload: &Value, changed_at: u64) -> Option<Self> {
        let op = match entity_type {
            "dir_state" => {
                let dir_id = DirId::parse(&entity_id).ok()?;
                let parent_id = DirId::parse(&string_field(payload, "parent_id")?).ok()?;
                let name = FileName::parse(&string_field(payload, "name")?).ok()?;
                let deleted = bool_field(payload, "deleted")?;
                let directory = if deleted { DirectoryState::Deleted { parent_id, name } } else { DirectoryState::Present { parent_id, name } };
                Some(SyncOp::DirState { dir_id, directory })
            }
            "book_added" => Some(SyncOp::BookAdded {
                title: payload.get("title").and_then(|v| v.as_str()).unwrap_or(&entity_id).to_string(),
                added_at: payload.get("added_at").and_then(|v| v.as_i64()).unwrap_or(changed_at as i64),
                book_id: BookId::parse(&entity_id).ok()?,
            }),
            "book_deleted" => Some(SyncOp::BookDeleted { book_id: BookId::parse(&entity_id).ok()? }),
            "book_dir_added" => Some(SyncOp::BookDirAdded {
                book_id: BookId::parse(&entity_id).ok()?,
                dir_id: DirId::parse(&string_field(payload, "dir_id")?).ok()?,
                file_name: FileName::parse(&string_field(payload, "file_name")?).ok()?,
                content_hash: ContentHash::parse(&string_field(payload, "content_hash")?).ok()?,
            }),
            "book_dir_removed" | "book_dir_deleted" => {
                Some(SyncOp::BookDirRemoved { book_id: BookId::parse(&entity_id).ok()?, dir_id: DirId::parse(&string_field(payload, "dir_id")?).ok()?, file_name: FileName::parse(&string_field(payload, "file_name")?).ok()? })
            }
            "reading_position_updated" => {
                let cfi = string_field(payload, "cfi")?;
                Some(SyncOp::ReadingPositionUpdated { book_id: BookId::parse(&entity_id).ok()?, cfi })
            }
            "book_opened" => Some(SyncOp::BookOpened { book_id: BookId::parse(&entity_id).ok()?, last_read: payload.get("last_read").and_then(|v| v.as_i64())? }),
            "book_metadata_updated" => Some(SyncOp::BookMetadataUpdated { book_id: BookId::parse(&entity_id).ok()?, title: string_field(payload, "title")?, author: string_field(payload, "author")? }),
            _ => None,
        }?;
        op.validate().ok()?;
        Some(op)
    }

    /// Validation that cannot be expressed by the wire representation alone.
    /// Servers call this before persistence so one malformed event can never
    /// become a permanent pull-cursor barrier.
    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        match self {
            Self::DirState { dir_id, .. } if dir_id.as_str() == "__root__" => Err(ProtocolValidationError::RootDirectoryMutation),
            Self::DirState { dir_id, directory } if dir_id == directory.parent_id() => Err(ProtocolValidationError::DirectorySelfParent),
            Self::BookAdded { added_at, .. } if *added_at < 0 => Err(ProtocolValidationError::NegativeBookAddedTimestamp),
            Self::BookAdded { title, .. } if !valid_metadata_text(title) => Err(ProtocolValidationError::InvalidBookTitle),
            Self::ReadingPositionUpdated { cfi, .. } if !valid_reading_position(cfi) => Err(ProtocolValidationError::InvalidReadingPosition),
            Self::BookOpened { last_read, .. } if *last_read < 0 => Err(ProtocolValidationError::NegativeLastReadTimestamp),
            Self::BookMetadataUpdated { title, .. } if !valid_metadata_text(title) => Err(ProtocolValidationError::InvalidMetadataTitle),
            Self::BookMetadataUpdated { author, .. } if !valid_metadata_text(author) => Err(ProtocolValidationError::InvalidMetadataAuthor),
            _ => Ok(()),
        }
    }

    /// Compatibility helper for callers that only need a yes/no answer.
    pub fn is_semantically_valid(&self) -> bool {
        self.validate().is_ok()
    }
}

fn valid_metadata_text(value: &str) -> bool {
    value.len() <= 4096 && !value.chars().any(char::is_control)
}

/// Protocol-level EPUB CFI validation shared by clients and the server.
pub fn valid_epub_cfi(value: &str) -> bool {
    if value.len() > 8 * 1024 || value.trim() != value || value.chars().any(char::is_control) {
        return false;
    }
    let Some(inner) = value.strip_prefix("epubcfi(").and_then(|value| value.strip_suffix(')')) else {
        return false;
    };
    if !inner.starts_with('/') || inner.len() == 1 {
        return false;
    }
    let mut square_depth = 0_u32;
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '^' {
            escaped = true;
            continue;
        }
        match character {
            '[' => square_depth = square_depth.saturating_add(1),
            ']' if square_depth == 0 => return false,
            ']' => square_depth -= 1,
            '(' | ')' => return false,
            _ => {}
        }
    }
    !escaped && square_depth == 0
}

/// Validates the synchronized position representation used by every supported
/// reader. EPUB retains its standard CFI representation; PDF uses a zero-based
/// page marker so the same LWW register can resume either format.
pub fn valid_reading_position(value: &str) -> bool {
    valid_epub_cfi(value) || pdf_page_from_reading_position(value).is_some()
}

pub fn pdf_page_from_reading_position(value: &str) -> Option<u32> {
    let page = value.strip_prefix("pdfpage(")?.strip_suffix(')')?;
    if page.is_empty() || (page.len() > 1 && page.starts_with('0')) {
        return None;
    }
    page.parse().ok()
}

pub fn pdf_page_reading_position(page_index: u32) -> String {
    format!("pdfpage({page_index})")
}

impl ServerChange {
    pub fn version_key(&self) -> VersionKey {
        VersionKey::new(self.hlc, self.op.conflict_rank(), self.actor_id.clone(), self.local_seq.get(), self.event_id)
    }
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}

fn bool_field(payload: &Value, key: &str) -> Option<bool> {
    let value = payload.get(key)?;
    value.as_bool().or_else(|| value.as_i64().map(|v| v != 0))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushChangesRequest {
    pub user_id: String,
    pub library_id: LibraryId,
    pub client_id: ClientId,
    pub changes: Vec<LocalChange>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushChangesResponse {
    #[serde(default)]
    pub handled_local_seqs: Vec<LocalSeq>,
    #[serde(default)]
    pub rejected: Vec<PushRejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushRejection {
    pub local_seq: LocalSeq,
    pub reason: PushRejectionReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushRejectionReason {
    HlcTooFarFuture,
    ValueOutsideStorageRange,
    InvalidPayload,
    MissingDependency,
}

impl PushRejectionReason {
    /// Whether the rejection is temporary, so the change should be retried later
    /// rather than dropped from the outbox. Clock-skew rejections are transient;
    /// other malformed changes are rejected during deserialization.
    pub fn is_transient(self) -> bool {
        matches!(self, PushRejectionReason::HlcTooFarFuture | PushRejectionReason::MissingDependency)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullChangesResponse {
    pub changes: Vec<ServerChange>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullQuery {
    pub user_id: String,
    pub library_id: LibraryId,
    pub client_id: ClientId,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_server_cursor")]
    pub since_server_seq: Option<ServerSeq>,
}

fn deserialize_server_cursor<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<ServerSeq>, D::Error> {
    match Option::<u64>::deserialize(deserializer)? {
        None | Some(0) => Ok(None),
        Some(value) => ServerSeq::new(value).map(Some).map_err(serde::de::Error::custom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_hash() -> ContentHash {
        ContentHash::parse(&"a".repeat(64)).unwrap()
    }

    fn dir_id(value: &str) -> DirId {
        DirId::parse(value).unwrap()
    }

    fn file_name(value: &str) -> FileName {
        FileName::parse(value).unwrap()
    }

    fn actor_id(value: &str) -> ActorId {
        ActorId::parse(value).unwrap()
    }

    fn variants() -> Vec<SyncOp> {
        vec![
            SyncOp::DirState { dir_id: dir_id("d1"), directory: DirectoryState::Present { parent_id: dir_id("__root__"), name: file_name("foo") } },
            SyncOp::DirState { dir_id: dir_id("d2"), directory: DirectoryState::Deleted { parent_id: dir_id("__root__"), name: file_name("gone") } },
            SyncOp::BookAdded { book_id: BookId::new(1), title: "Title".into(), added_at: 1234 },
            SyncOp::BookDeleted { book_id: BookId::new(1) },
            SyncOp::BookDirAdded { book_id: BookId::new(1), dir_id: dir_id("d1"), file_name: file_name("f.epub"), content_hash: content_hash() },
            SyncOp::BookDirRemoved { book_id: BookId::new(1), dir_id: dir_id("d1"), file_name: file_name("f.epub") },
            SyncOp::ReadingPositionUpdated { book_id: BookId::new(1), cfi: "epubcfi(/6/4)".into() },
            SyncOp::BookOpened { book_id: BookId::new(1), last_read: 9999 },
            SyncOp::BookMetadataUpdated { book_id: BookId::new(1), title: "Updated title".into(), author: "Author".into() },
        ]
    }

    #[test]
    fn round_trip_payload_and_parts() {
        for op in variants() {
            let event_type = op.event_type();
            let entity_id = op.entity_id().to_string();
            let payload = op.payload();
            let reconstructed = SyncOp::from_event_parts(event_type, entity_id, &payload, 0).unwrap_or_else(|| panic!("from_event_parts returned None for {event_type}"));
            assert_eq!(op, reconstructed, "round-trip mismatch for {event_type}");
        }
    }

    #[test]
    fn pdf_page_positions_round_trip_and_reject_noncanonical_values() {
        let value = pdf_page_reading_position(42);
        assert_eq!(value, "pdfpage(42)");
        assert_eq!(pdf_page_from_reading_position(&value), Some(42));
        assert!(valid_reading_position(&value));
        assert!(valid_reading_position("epubcfi(/6/4)"));
        assert_eq!(pdf_page_from_reading_position("pdfpage(01)"), None);
        assert_eq!(pdf_page_from_reading_position("pdfpage(-1)"), None);
    }

    #[test]
    fn event_type_matches_serde_tag() {
        for op in variants() {
            let value = serde_json::to_value(&op).expect("serialize");
            let tag = value.get("type").and_then(|v| v.as_str()).expect("serde tag present");
            assert_eq!(op.event_type(), tag, "event_type() must match serde rename_all = snake_case");
            assert!(SyncOp::is_known_event_type(op.event_type()));
        }
        assert!(SyncOp::is_known_event_type("book_dir_deleted"));
        assert!(!SyncOp::is_known_event_type("future_event"));
    }

    #[test]
    fn sync_wire_keeps_book_ids_as_decimal_strings() {
        let value = serde_json::to_value(SyncOp::BookDeleted { book_id: BookId::new(42) }).expect("serialize");
        assert_eq!(value.get("book_id"), Some(&Value::String("42".to_string())));
        assert_eq!(serde_json::from_value::<SyncOp>(value).expect("deserialize"), SyncOp::BookDeleted { book_id: BookId::new(42) });
    }

    #[test]
    fn book_id_json_uses_the_canonical_string_and_reads_legacy_numbers() {
        assert_eq!(serde_json::to_value(BookId::new(42)).expect("serialize"), json!("42"));
        assert_eq!(serde_json::from_value::<BookId>(json!(42)).expect("deserialize legacy number"), BookId::new(42));
    }

    #[test]
    fn storage_codecs_round_trip_without_exposing_the_inner_representation() {
        let book_id = BookId::new(u64::MAX);
        assert_eq!(BookId::parse(&book_id.canonical()), Ok(book_id));
        let content_hash = content_hash();
        assert_eq!(serde_json::from_value::<ContentHash>(serde_json::to_value(&content_hash).unwrap()).unwrap(), content_hash);
        assert!(ContentHash::parse("legacy-or-blake3-key").is_err());
        assert!(ContentHash::parse(&"A".repeat(64)).is_err());
        assert!(serde_json::from_value::<ContentHash>(json!("legacy-or-blake3-key")).is_err());
        assert!(ActorId::parse("").is_err());
        assert!(DirId::parse("a/b").is_err());
        assert!(FileName::parse("..").is_err());
        assert!(LocalSeq::new(0).is_err());
        assert!(ServerSeq::new(i64::MAX as u64 + 1).is_err());
    }

    #[test]
    fn wire_lifecycle_requires_the_fields_owned_by_each_stage() {
        let local = LocalChange { event_id: EventId::new(), op: SyncOp::BookDeleted { book_id: BookId::new(1) }, changed_at: 1, hlc: Hlc::default(), local_seq: LocalSeq::new(1).unwrap() };
        let local_json = serde_json::to_value(&local).unwrap();
        assert!(serde_json::from_value::<ServerChange>(local_json).is_err());
        assert!(
            serde_json::from_value::<LocalChange>(json!({
                "op": { "type": "book_deleted", "book_id": "1" },
                "changed_at": 1,
                "hlc": { "pt": 1, "l": 0 },
                "local_seq": 1
            }))
            .is_err(),
            "a retry-safe mutation identity is mandatory on the wire"
        );

        let query = PullQuery { user_id: "user".into(), library_id: LibraryId::parse("library").unwrap(), client_id: ClientId::parse("client").unwrap(), since_server_seq: None };
        assert!(serde_json::to_value(query).unwrap().get("since_server_seq").is_none());
        let legacy_zero = serde_json::from_value::<PullQuery>(json!({ "user_id": "user", "library_id": "library", "client_id": "client", "since_server_seq": 0 })).unwrap();
        assert_eq!(legacy_zero.since_server_seq, None);
    }

    #[test]
    fn book_id_derives_from_the_first_eight_blake3_bytes() {
        let content_hash = ContentHash::parse("d75c7dd40485dd0ff2be7cfcb0cf2708634a6f1718c5e694345d513a89f4af5b").unwrap();
        assert_eq!(BookId::from_content_hash(&content_hash), BookId::new(1143216136213322967));
    }

    #[test]
    fn singleton_list_matches_is_singleton() {
        for op in variants() {
            let by_const = SINGLETON_EVENT_TYPES.contains(&op.event_type());
            assert_eq!(op.is_singleton(), by_const);
        }
    }

    #[test]
    fn version_key_totally_orders_equal_hlc_changes() {
        let local = LocalChange {
            event_id: EventId::new(),
            op: SyncOp::BookDirAdded { book_id: BookId::new(1), dir_id: dir_id("d"), file_name: file_name("f"), content_hash: content_hash() },
            changed_at: 0,
            hlc: Hlc { pt: 10, l: 0 },
            local_seq: LocalSeq::new(1).unwrap(),
        };
        let mut add = ServerChange { change: local, actor_id: actor_id("actor-a"), server_seq: ServerSeq::new(1).unwrap() };
        let mut other_add = add.clone();
        other_add.actor_id = actor_id("actor-b");
        assert!(other_add.version_key() > add.version_key());

        add.change.local_seq = LocalSeq::new(2).unwrap();
        other_add.actor_id = add.actor_id.clone();
        other_add.change.local_seq = LocalSeq::new(3).unwrap();
        assert!(other_add.version_key() > add.version_key());

        let remove = ServerChange { change: LocalChange { op: SyncOp::BookDirRemoved { book_id: BookId::new(1), dir_id: dir_id("d"), file_name: file_name("f") }, ..add.change.clone() }, ..add.clone() };
        assert!(remove.version_key() > add.version_key());
    }

    #[test]
    fn missing_dependencies_are_retryable_without_dropping_the_outbox_event() {
        assert!(PushRejectionReason::MissingDependency.is_transient());
        assert!(!PushRejectionReason::InvalidPayload.is_transient());
    }

    #[test]
    fn directory_semantics_reserve_the_root_and_reject_self_parenting() {
        let root_mutation = SyncOp::DirState { dir_id: dir_id("__root__"), directory: DirectoryState::Present { parent_id: dir_id("shelf"), name: file_name("Root") } };
        let self_parent = SyncOp::DirState { dir_id: dir_id("shelf"), directory: DirectoryState::Present { parent_id: dir_id("shelf"), name: file_name("Shelf") } };
        assert_eq!(root_mutation.validate(), Err(ProtocolValidationError::RootDirectoryMutation));
        assert_eq!(self_parent.validate(), Err(ProtocolValidationError::DirectorySelfParent));
    }

    #[test]
    fn protocol_validation_reports_each_invalid_payload_domain() {
        let invalid = [
            (SyncOp::BookAdded { book_id: BookId::new(1), title: "Title".into(), added_at: -1 }, ProtocolValidationError::NegativeBookAddedTimestamp),
            (SyncOp::BookAdded { book_id: BookId::new(1), title: "bad\ntitle".into(), added_at: 1 }, ProtocolValidationError::InvalidBookTitle),
            (SyncOp::ReadingPositionUpdated { book_id: BookId::new(1), cfi: "not-a-cfi".into() }, ProtocolValidationError::InvalidReadingPosition),
            (SyncOp::BookOpened { book_id: BookId::new(1), last_read: -1 }, ProtocolValidationError::NegativeLastReadTimestamp),
            (SyncOp::BookMetadataUpdated { book_id: BookId::new(1), title: "bad\ntitle".into(), author: "Author".into() }, ProtocolValidationError::InvalidMetadataTitle),
            (SyncOp::BookMetadataUpdated { book_id: BookId::new(1), title: "Title".into(), author: "bad\nauthor".into() }, ProtocolValidationError::InvalidMetadataAuthor),
        ];

        for (op, expected) in invalid {
            assert_eq!(op.validate(), Err(expected));
            assert!(!op.is_semantically_valid());
            assert!(SyncOp::from_event_parts(op.event_type(), op.entity_id().into_owned(), &op.payload(), 0).is_none());
        }
    }
}
