use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TorrentInput {
    Magnet(String),
    File(serde_bytes::ByteBuf),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TorrentStatus {
    pub info_hash: String,
    pub name: String,
    pub progress_bytes: u64,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub imported: bool,
    pub keep_seeding: bool,
    pub error: Option<String>,
}
