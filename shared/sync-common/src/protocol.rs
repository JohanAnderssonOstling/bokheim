//! Transport-independent validation for synchronization pages.

use crate::ContentHash;
use std::error::Error;
use std::fmt::{Display, Formatter};
use crate::transport::encode;
use crate::{MAX_PUSH_BATCH_BYTES, MAX_PUSH_MUTATIONS, ReplicaSeq};
use crate::{LibraryRevision, MutationRejection, MutationRejectionReason, PullStateResponse, PushMutationsResponse, SyncCursor, OpaquePublication};
use std::collections::{HashSet, VecDeque};

#[derive(Debug)]
struct EncodedMutation {
    publication: OpaquePublication,
    bytes: usize,
}

/// Wire mutations prepared for bounded, order-preserving push requests.
///
/// A mutation larger than a complete batch is retained separately so it does
/// not block later mutations. The caller remains responsible for deciding how
/// to surface or quarantine those untransmittable mutations.
#[derive(Debug, Default)]
pub struct PushBatcher {
    ready: VecDeque<EncodedMutation>,
    untransmittable: Vec<ReplicaSeq>,
}

impl PushBatcher {
    pub fn new(mutations: Vec<OpaquePublication>) -> Result<Self, crate::transport::WireError> {
        let mut batcher = Self { ready: VecDeque::with_capacity(mutations.len()), untransmittable: Vec::new() };
        for mutation in mutations {
            let bytes = encode(&mutation)?.len();
            if bytes > MAX_PUSH_BATCH_BYTES {
                batcher.untransmittable.push(mutation.publisher_seq);
            } else {
                batcher.ready.push_back(EncodedMutation { publication: mutation, bytes });
            }
        }
        Ok(batcher)
    }

    pub fn untransmittable(&self) -> &[ReplicaSeq] {
        &self.untransmittable
    }

    pub fn is_empty(&self) -> bool {
        self.ready.is_empty()
    }

    /// Leave fields of failed creations in the durable outbox for a later
    /// exchange. Sending them in a later page would look like standalone
    /// absent-book edits, which the server correctly acknowledges as no-ops.
    pub fn defer_book_fields(&mut self, books: &[ContentHash]) {
        self.ready.retain(|row| !row.publication.state.value.book.is_some_and(|book| books.contains(&book)));
    }

    pub fn next_batch(&mut self) -> Vec<OpaquePublication> {
        let mut batch = Vec::with_capacity(MAX_PUSH_MUTATIONS.min(self.ready.len()));
        let mut bytes = 0_usize;
        while batch.len() < MAX_PUSH_MUTATIONS {
            let Some(next) = self.ready.front() else { break };
            if !batch.is_empty() && bytes.saturating_add(next.bytes) > MAX_PUSH_BATCH_BYTES {
                break;
            }
            bytes = bytes.saturating_add(next.bytes);
            batch.push(self.ready.pop_front().expect("the inspected mutation is still queued").publication);
        }
        batch
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PushResponseError {
    InvalidAcknowledgments(String),
    PermanentRejection { publisher_seq: ReplicaSeq, reason: MutationRejectionReason },
}

impl Display for PushResponseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAcknowledgments(reason) => write!(formatter, "invalid mutation acknowledgement response: {reason}"),
            Self::PermanentRejection { publisher_seq, reason } => write!(formatter, "server permanently rejected mutation {publisher_seq}: {reason:?}"),
        }
    }
}

impl Error for PushResponseError {}

#[derive(Debug, PartialEq, Eq)]
pub struct PullBatchError(String);

impl Display for PullBatchError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid pull batch: {}", self.0)
    }
}

impl Error for PullBatchError {}

fn revision(value: Option<LibraryRevision>) -> u64 {
    value.map(LibraryRevision::get).unwrap_or(0)
}

pub fn validate_pull_batch(response: &PullStateResponse, cursor: SyncCursor) -> Result<(), PullBatchError> {
    let next_lifecycle = revision(response.next_cursor.lifecycle_revision);
    let next_field = revision(response.next_cursor.field_revision);
    let mut observed_lifecycle = revision(cursor.lifecycle_revision);
    let mut observed_field = revision(cursor.field_revision);
    let mut fields_started = false;
    for mutation in &response.mutations {
        let revision = mutation.revision.get();
        let lifecycle = mutation.state.value.address.kind == "book_lifecycle";
        if lifecycle && fields_started {
            return Err(PullBatchError("a book lifecycle follows a field in the page".into()));
        }
        fields_started |= !lifecycle;
        let channel = if lifecycle { &mut observed_lifecycle } else { &mut observed_field };
        if revision <= *channel {
            return Err(PullBatchError(format!("library revision {revision} does not advance its channel")));
        }
        if !lifecycle && revision > next_lifecycle {
            return Err(PullBatchError(format!("field revision {revision} precedes the lifecycles it may depend on")));
        }
        *channel = revision;
    }
    if next_lifecycle < observed_lifecycle || next_field < observed_field || next_field > next_lifecycle {
        return Err(PullBatchError("next cursor precedes an observed revision or passes the lifecycle cursor".into()));
    }
    if response.has_more && response.mutations.is_empty() {
        return Err(PullBatchError("a non-final page cannot be empty".into()));
    }
    let ends_in_lifecycles = !fields_started;
    if response.has_more && (next_field != observed_field || (ends_in_lifecycles && next_lifecycle != observed_lifecycle)) {
        return Err(PullBatchError("a non-final page cursor must end at its last delivered revision".into()));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedPushResponse {
    pub accepted: Vec<ReplicaSeq>,
    pub issues: Vec<MutationIssue>,
}

/// A local mutation that still needs attention after an otherwise successful
/// synchronization exchange. Keeping these cases in one collection prevents
/// callers from having to keep parallel vectors aligned.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MutationIssue {
    /// The server explicitly asked the client to submit this mutation later.
    Retryable(MutationRejection),
    /// The server conclusively rejected this mutation; it has left the outbox.
    Rejected(MutationRejection),
    /// The mutation cannot fit in a legal request and remains in the outbox.
    Untransmittable(ReplicaSeq),
}

impl ValidatedPushResponse {
    /// Mutations that must leave the durable outbox, whether accepted or
    /// permanently rejected. Retrying either class cannot change the result.
    pub fn completed_sequences(&self) -> Vec<ReplicaSeq> {
        self.accepted
            .iter()
            .copied()
            .chain(self.issues.iter().filter_map(|issue| match issue {
                MutationIssue::Rejected(rejection) => Some(rejection.publisher_seq),
                MutationIssue::Retryable(_) | MutationIssue::Untransmittable(_) => None,
            }))
            .collect()
    }
}

pub fn validate_push_response(requested: &[ReplicaSeq], response: &PushMutationsResponse) -> Result<ValidatedPushResponse, PushResponseError> {
    let requested_ids: HashSet<_> = requested.iter().copied().collect();
    if requested_ids.len() != requested.len() {
        return Err(PushResponseError::InvalidAcknowledgments("submitted mutation identities are not unique".into()));
    }
    let mut accounted = HashSet::new();
    let mut issues = Vec::new();
    for rejection in &response.rejected {
        if !requested_ids.contains(&rejection.publisher_seq) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server rejected unsubmitted mutation {}", rejection.publisher_seq)));
        }
        if !accounted.insert(rejection.publisher_seq) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server accounted for mutation {} more than once", rejection.publisher_seq)));
        }
        if rejection.reason.is_transient() {
            issues.push(MutationIssue::Retryable(rejection.clone()));
        } else {
            issues.push(MutationIssue::Rejected(rejection.clone()));
        }
    }
    let accepted = requested.iter().copied().filter(|seq| !accounted.contains(seq)).collect();
    Ok(ValidatedPushResponse { accepted, issues })
}
