//! Durable device-local failures. A paused file is stat'ed, not reparsed,
//! until its input changes or the user explicitly requests another attempt.
use super::{read_json, write_json, AutomaticExport, LibrarySession, Receipt};
use crate::BackendError;
use library_model::{AnnotationExportDestination, AnnotationExportFailure, AnnotationExportFailureKind, AnnotationExportFormat, AutomaticAnnotationExportOutput, AutomaticAnnotationExportState, LibraryUpdate};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ExportState {
    pub error: Option<String>,
    pub failures: Vec<BlockedOutput>,
    pub last_success_at: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BlockedOutput {
    pub output: AutomaticAnnotationExportOutput,
    fingerprint: String,
    stamp: Option<FileStamp>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    length: u64,
    modified: u128,
    readonly: bool,
    file: bool,
    symlink: bool,
}

pub(super) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().try_into().unwrap_or(u64::MAX)
}
fn stamp(session: &LibrarySession, destination: &AnnotationExportDestination) -> Option<FileStamp> {
    let path = if destination.format == AnnotationExportFormat::Pdf { session.assets.native_library_root().join(destination.path.to_string_lossy().trim_start_matches('/')) } else { destination.path.clone() };
    let metadata = fs::symlink_metadata(path).ok()?;
    Some(FileStamp {
        length: metadata.len(),
        modified: metadata.modified().ok()?.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(),
        readonly: metadata.permissions().readonly(),
        file: metadata.is_file(),
        symlink: metadata.file_type().is_symlink(),
    })
}
pub(super) fn load(session: &LibrarySession) -> Result<ExportState, BackendError> {
    let automatic = &session.runtime.automatic_export;
    if automatic.status.borrow().is_none() {
        let state = match read_json(&session.db.path().with_extension("annotation-export-status.json")) {
            Ok(state) => state.unwrap_or_default(),
            Err(error) => {
                // This is a replaceable local cache, not annotation data. A
                // damaged status file must never prevent an annotation export.
                automatic.status_dirty.set(true);
                ExportState { error: Some(format!("Previous export status could not be read: {error}")), ..Default::default() }
            }
        };
        automatic.status.replace(Some(state));
    }
    Ok(automatic.status.borrow().as_ref().cloned().unwrap_or_default())
}
pub(super) fn publish(session: &LibrarySession, state: ExportState) -> Result<(), BackendError> {
    let automatic = &session.runtime.automatic_export;
    if automatic.status.borrow().as_ref() != Some(&state) {
        automatic.status.replace(Some(state.clone()));
        automatic.status_dirty.set(true);
        session.events.publish_update(LibraryUpdate::AnnotationExportStatusChanged);
    }
    if automatic.status_dirty.get() {
        write_json(&session.db.path().with_extension("annotation-export-status.json"), &state)?;
        automatic.status_dirty.set(false);
    }
    Ok(())
}
pub(super) fn global_error(session: &LibrarySession, error: String) {
    let result = load(session).and_then(|mut state| {
        state.error = Some(error);
        publish(session, state)
    });
    if let Err(error) = result {
        log::warn!("could not persist automatic annotation export status: {error}");
    }
}
pub(super) fn backoff(attempt: u32) -> u64 {
    30_000_u64.saturating_mul(1_u64 << attempt.saturating_sub(1).min(4)).min(300_000)
}
impl BlockedOutput {
    /// Check a paused destination without loading its book's annotations.
    pub fn due(&self, session: &LibrarySession, now: u64) -> bool {
        match self.output.state {
            AutomaticAnnotationExportState::Waiting => true,
            AutomaticAnnotationExportState::NeedsAttention => self.stamp != stamp(session, &self.output.destination),
            AutomaticAnnotationExportState::Retrying => now < self.output.last_attempt_at || self.output.next_retry_at.is_none_or(|time| time <= now) || self.stamp != stamp(session, &self.output.destination),
        }
    }

    pub fn ready(&self, session: &LibrarySession, receipt: &Receipt, now: u64) -> bool {
        (self.output.state == AutomaticAnnotationExportState::NeedsAttention && self.fingerprint != receipt.fingerprint) || self.due(session, now)
    }
}
impl ExportState {
    pub fn success(&mut self, destination: &AnnotationExportDestination, now: u64) {
        self.failures.retain(|entry| !super::same_destination(&entry.output.destination, destination));
        self.last_success_at = Some(now);
    }
    pub fn failed(&mut self, session: &LibrarySession, receipt: &Receipt, title: String, failure: AnnotationExportFailure, message: String, last_success_at: Option<u64>, now: u64) {
        let attempts = self.failures.iter().find(|entry| super::same_destination(&entry.output.destination, &receipt.destination)).map_or(1, |entry| entry.output.attempts.saturating_add(1));
        let state = match failure.kind {
            AnnotationExportFailureKind::Busy | AnnotationExportFailureKind::Unavailable => AutomaticAnnotationExportState::Waiting,
            AnnotationExportFailureKind::InvalidData | AnnotationExportFailureKind::Identity | AnnotationExportFailureKind::Unsupported | AnnotationExportFailureKind::LimitExceeded => AutomaticAnnotationExportState::NeedsAttention,
            _ => AutomaticAnnotationExportState::Retrying,
        };
        self.failures.retain(|entry| !super::same_destination(&entry.output.destination, &receipt.destination));
        self.failures.push(BlockedOutput {
            output: AutomaticAnnotationExportOutput {
                destination: receipt.destination.clone(),
                book_title: title,
                state,
                message,
                failure,
                attempts,
                last_attempt_at: now,
                next_retry_at: (state == AutomaticAnnotationExportState::Retrying).then(|| now.saturating_add(backoff(attempts))),
                last_success_at,
            },
            fingerprint: receipt.fingerprint.clone(),
            stamp: stamp(session, &receipt.destination),
        });
    }
}
impl LibrarySession {
    pub(crate) fn retry_automatic_annotation_export(&self) -> Result<(), BackendError> {
        if self.automatic_annotation_export()?.options.is_none() {
            return Err(BackendError::message("automatic annotation export is not enabled"));
        }
        let automatic: &AutomaticExport = &self.runtime.automatic_export;
        if let Some(active) = automatic.active.borrow().as_ref() {
            active.cancel();
        }
        automatic.generation.set(automatic.generation.get().wrapping_add(1));
        let mut state = load(self)?;
        state.error = None;
        for entry in &mut state.failures {
            entry.output.state = AutomaticAnnotationExportState::Retrying;
            entry.output.next_retry_at = Some(0);
        }
        // Wake even if persisting status is temporarily unavailable.
        self.events.annotation_exports.notify_one();
        publish(self, state)
    }
}
