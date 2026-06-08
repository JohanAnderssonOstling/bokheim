use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Event types that collapse to one row per `(library_id, entity_id, event_type)`
/// under LWW semantics. All other event types are append-only and deduped by
/// `(library_id, client_id, client_seq)`.
///
/// The Postgres partial indexes `idx_library_events_lww` / `idx_library_events_dedup`
/// in migration 0001 hardcode the same list — keep them in sync.
pub const SINGLETON_EVENT_TYPES: &[&str] = &["reading_position_updated", "book_opened"];

/// Renders `SINGLETON_EVENT_TYPES` as a parenthesised SQL list, e.g.
/// `('reading_position_updated', 'book_opened')`. Use to interpolate into
/// `WHERE event_type IN ...` / `NOT IN ...` so the SQL matches the constant.
pub fn singleton_event_types_sql_list() -> String {
    let quoted: Vec<String> = SINGLETON_EVENT_TYPES.iter().map(|s| format!("'{}'", s)).collect();
    format!("({})", quoted.join(", "))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncOp {
    DirAdded { dir_id: String, parent_id: String, name: String },
    DirMoved { dir_id: String, new_parent_id: String, new_name: String },
    DirDeleted { dir_id: String },
    BookAdded { book_id: String, title: String, added_at: i64 },
    BookDeleted { book_id: String },
    BookDirAdded { book_id: String, dir_id: String, file_name: String, content_hash: String },
    BookDirRemoved { book_id: String, dir_id: String, file_name: String },
    ReadingPositionUpdated { book_id: String, cfi: String },
    BookOpened { book_id: String, last_read: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Change {
    pub op: SyncOp,
    pub changed_at: u64,
    #[serde(default)]
    pub hlc: Hlc,
    /// Sequence number assigned by the creating device (local sequence).
    #[serde(default)]
    pub local_seq: Option<u64>,
    /// Sequence number assigned by server (for ordering across devices).
    #[serde(default)]
    pub server_seq: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Hlc {
    pub pt: u64,
    pub l: u64,
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
    pub fn event_type(&self) -> &'static str {
        match self {
            SyncOp::DirAdded { .. } => "dir_added",
            SyncOp::DirMoved { .. } => "dir_moved",
            SyncOp::DirDeleted { .. } => "dir_deleted",
            SyncOp::BookAdded { .. } => "book_added",
            SyncOp::BookDeleted { .. } => "book_deleted",
            SyncOp::BookDirAdded { .. } => "book_dir_added",
            SyncOp::BookDirRemoved { .. } => "book_dir_removed",
            SyncOp::ReadingPositionUpdated { .. } => "reading_position_updated",
            SyncOp::BookOpened { .. } => "book_opened",
        }
    }

    pub fn entity_id(&self) -> &str {
        match self {
            SyncOp::DirAdded { dir_id, .. } | SyncOp::DirMoved { dir_id, .. } | SyncOp::DirDeleted { dir_id, .. } => dir_id,
            SyncOp::BookAdded { book_id, .. }
            | SyncOp::BookDeleted { book_id }
            | SyncOp::BookDirAdded { book_id, .. }
            | SyncOp::BookDirRemoved { book_id, .. }
            | SyncOp::ReadingPositionUpdated { book_id, .. }
            | SyncOp::BookOpened { book_id, .. } => book_id,
        }
    }

    pub fn is_singleton(&self) -> bool {
        SINGLETON_EVENT_TYPES.contains(&self.event_type())
    }

    pub fn payload(&self) -> Value {
        match self {
            SyncOp::DirAdded { parent_id, name, .. } => json!({ "parent_id": parent_id, "name": name }),
            SyncOp::DirMoved { new_parent_id, new_name, .. } => json!({ "new_parent_id": new_parent_id, "new_name": new_name }),
            SyncOp::DirDeleted { .. } => json!({}),
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
        }
    }

    pub fn from_event_parts(entity_type: &str, entity_id: String, payload: &Value, changed_at: u64) -> Option<Self> {
        match entity_type {
            "dir_added" => Some(SyncOp::DirAdded {
                dir_id: entity_id,
                parent_id: string_field(payload, "parent_id")?,
                name: string_field(payload, "name")?,
            }),
            "dir_moved" => Some(SyncOp::DirMoved {
                dir_id: entity_id,
                new_parent_id: string_field(payload, "new_parent_id")?,
                new_name: string_field(payload, "new_name")?,
            }),
            "dir_deleted" => Some(SyncOp::DirDeleted { dir_id: entity_id }),
            "book_added" => Some(SyncOp::BookAdded {
                title: payload.get("title").and_then(|v| v.as_str()).unwrap_or(&entity_id).to_string(),
                added_at: payload.get("added_at").and_then(|v| v.as_i64()).unwrap_or(changed_at as i64),
                book_id: entity_id,
            }),
            "book_deleted" => Some(SyncOp::BookDeleted { book_id: entity_id }),
            "book_dir_added" => Some(SyncOp::BookDirAdded {
                book_id: entity_id,
                dir_id: string_field(payload, "dir_id")?,
                file_name: string_field(payload, "file_name")?,
                // Optional: legacy events predate content-hash propagation.
                content_hash: string_field(payload, "content_hash").unwrap_or_default(),
            }),
            "book_dir_removed" | "book_dir_deleted" => Some(SyncOp::BookDirRemoved {
                book_id: entity_id,
                dir_id: string_field(payload, "dir_id")?,
                file_name: string_field(payload, "file_name")?,
            }),
            "reading_position_updated" => Some(SyncOp::ReadingPositionUpdated { book_id: entity_id, cfi: string_field(payload, "cfi")? }),
            "book_opened" => Some(SyncOp::BookOpened { book_id: entity_id, last_read: payload.get("last_read").and_then(|v| v.as_i64())? }),
            _ => None,
        }
    }
}

fn string_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushChangesRequest {
    pub user_id: String,
    pub library_id: String,
    pub client_id: String,
    pub changes: Vec<Change>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushChangesResponse {
    #[serde(default)]
    pub handled_local_seqs: Vec<u64>,
    #[serde(default)]
    pub rejected: Vec<PushRejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushRejection {
    pub local_seq: u64,
    pub reason: PushRejectionReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushRejectionReason {
    MissingLocalSeq,
    HlcTooFarFuture,
}

impl PushRejectionReason {
    /// Whether the rejection is temporary, so the change should be retried later
    /// rather than dropped from the outbox. Clock-skew rejections are transient;
    /// malformed changes (missing local_seq) are permanent.
    pub fn is_transient(self) -> bool {
        matches!(self, PushRejectionReason::HlcTooFarFuture)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullChangesResponse {
    pub changes: Vec<Change>,
}

#[derive(Debug, Deserialize)]
pub struct PullQuery {
    pub user_id: String,
    pub library_id: String,
    pub client_id: String,
    #[serde(default)]
    pub since_server_seq: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variants() -> Vec<SyncOp> {
        vec![
            SyncOp::DirAdded { dir_id: "d1".into(), parent_id: "__root__".into(), name: "foo".into() },
            SyncOp::DirMoved { dir_id: "d1".into(), new_parent_id: "d2".into(), new_name: "bar".into() },
            SyncOp::DirDeleted { dir_id: "d1".into() },
            SyncOp::BookAdded { book_id: "b1".into(), title: "Title".into(), added_at: 1234 },
            SyncOp::BookDeleted { book_id: "b1".into() },
            SyncOp::BookDirAdded { book_id: "b1".into(), dir_id: "d1".into(), file_name: "f.epub".into(), content_hash: "abc".into() },
            SyncOp::BookDirRemoved { book_id: "b1".into(), dir_id: "d1".into(), file_name: "f.epub".into() },
            SyncOp::ReadingPositionUpdated { book_id: "b1".into(), cfi: "epubcfi(/6/4)".into() },
            SyncOp::BookOpened { book_id: "b1".into(), last_read: 9999 },
        ]
    }

    #[test]
    fn round_trip_payload_and_parts() {
        for op in variants() {
            let event_type = op.event_type();
            let entity_id = op.entity_id().to_string();
            let payload = op.payload();
            let reconstructed = SyncOp::from_event_parts(event_type, entity_id, &payload, 0)
                .unwrap_or_else(|| panic!("from_event_parts returned None for {event_type}"));
            assert_eq!(op, reconstructed, "round-trip mismatch for {event_type}");
        }
    }

    #[test]
    fn event_type_matches_serde_tag() {
        for op in variants() {
            let value = serde_json::to_value(&op).expect("serialize");
            let tag = value.get("type").and_then(|v| v.as_str()).expect("serde tag present");
            assert_eq!(op.event_type(), tag, "event_type() must match serde rename_all = snake_case");
        }
    }

    #[test]
    fn singleton_list_matches_is_singleton() {
        for op in variants() {
            let by_const = SINGLETON_EVENT_TYPES.contains(&op.event_type());
            assert_eq!(op.is_singleton(), by_const);
        }
    }
}
