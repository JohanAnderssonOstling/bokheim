//! Import failures. Presentation and localization belong to the UI.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportFailureKind {
    File,
    Folder,
}

/// A classified failure, with its source path and optional diagnostic detail.
/// Diagnostics are for logging/support; they are not a user-facing message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportFailure {
    pub kind: ImportFailureKind,
    pub path: Vec<String>,
    pub diagnostic: Option<String>,
    #[serde(default)]
    pub failure: Option<client_runtime::Failure>,
}
impl ImportFailure {
    pub fn operation(kind: ImportFailureKind, path: &[String], diagnostic: impl Into<String>) -> Self {
        Self { kind, path: path.to_vec(), diagnostic: Some(diagnostic.into()), failure: None }
    }
}
