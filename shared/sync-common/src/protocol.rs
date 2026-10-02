//! Transport-independent validation for synchronization pages.

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
    pub fn defer_book_fields(&mut self, books: &[String]) {
        self.ready.retain(|row| !row.publication.state.value.book.is_some_and(|book| books.iter().any(|hash| hash == book.as_str())));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConflictRank, OpaqueStateEntry, Publication, ReplicaId, StateAddress, VersionKey, Versioned};

    fn server_mutation(kind: &str, revision: u64) -> crate::StateDelivery {
        crate::fixture_delivery(Publication { publisher_seq: ReplicaSeq::new(1).unwrap(), state: Versioned { version: VersionKey::new(1, ConflictRank::Update, ReplicaId::nil(), ReplicaSeq::new(1).unwrap()), value: OpaqueStateEntry { address: StateAddress { kind: kind.to_owned(), entity_key: "key".to_owned(), entity_subkey: String::new() }, bytes: Vec::new(), book: None } } }, ReplicaId::nil(), LibraryRevision::new(revision).unwrap())
    }

    fn wire_mutation(sequence: u64, value_bytes: usize) -> OpaquePublication {
        Publication { publisher_seq: ReplicaSeq::new(sequence).unwrap(), state: Versioned { version: VersionKey::new(sequence, ConflictRank::Update, ReplicaId::nil(), ReplicaSeq::new(sequence).unwrap()), value: OpaqueStateEntry { address: StateAddress { kind: "annotation".to_owned(), entity_key: sequence.to_string(), entity_subkey: String::new() }, bytes: vec![0; value_bytes], book: None } } }
    }

    #[test]
    fn push_batcher_preserves_order_and_sets_aside_untransmittable_mutations() {
        let first = wire_mutation(1, 1);
        let oversized = wire_mutation(2, MAX_PUSH_BATCH_BYTES + 1);
        let last = wire_mutation(3, 1);
        let oversized_id = oversized.publisher_seq;
        let mut batcher = PushBatcher::new(vec![first.clone(), oversized, last.clone()]).unwrap();

        assert_eq!(batcher.untransmittable(), &[oversized_id]);
        assert_eq!(batcher.next_batch().iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>(), vec![first.publisher_seq, last.publisher_seq]);
        assert!(batcher.is_empty());
    }

    #[test]
    fn push_batcher_limits_both_count_and_encoded_bytes() {
        let mut count_limited = PushBatcher::new((1..=MAX_PUSH_MUTATIONS as u64 + 1).map(|sequence| wire_mutation(sequence, 1)).collect()).unwrap();
        assert_eq!(count_limited.next_batch().len(), MAX_PUSH_MUTATIONS);
        assert_eq!(count_limited.next_batch().len(), 1);

        let mut byte_limited = PushBatcher::new((1..=100).map(|sequence| wire_mutation(sequence, 64 * 1024)).collect()).unwrap();
        let batch = byte_limited.next_batch();
        let encoded_bytes = batch.iter().map(|mutation| encode(mutation).unwrap().len()).sum::<usize>();
        assert!(!batch.is_empty());
        assert!(batch.len() < 100);
        assert!(encoded_bytes <= MAX_PUSH_BATCH_BYTES);
        assert!(!byte_limited.is_empty());
    }

    #[test]
    fn identity_acknowledgements_do_not_depend_on_sequence_prefixes() {
        let first = ReplicaSeq::new(1).unwrap();
        let second = ReplicaSeq::new(19).unwrap();
        let response = PushMutationsResponse { rejected: Vec::new() };
        let validated = validate_push_response(&[first, second], &response).unwrap();
        assert_eq!(validated.accepted, vec![first, second]);
    }

    #[test]
    fn every_requested_identity_is_partitioned_by_rejection_lifetime() {
        let accepted = ReplicaSeq::new(1).unwrap();
        let transient = ReplicaSeq::new(5).unwrap();
        let first_permanent = ReplicaSeq::new(17).unwrap();
        let second_permanent = ReplicaSeq::new(21).unwrap();
        let response = PushMutationsResponse {
            rejected: vec![
                MutationRejection { publisher_seq: transient, reason: MutationRejectionReason::TimestampTooFarFuture },
                MutationRejection { publisher_seq: first_permanent, reason: MutationRejectionReason::InvalidPayload },
                MutationRejection { publisher_seq: second_permanent, reason: MutationRejectionReason::InvalidPayload },
            ],
        };

        let validated = validate_push_response(&[accepted, transient, first_permanent, second_permanent], &response).unwrap();
        assert_eq!(validated.accepted, vec![accepted]);
        assert_eq!(
            validated.issues,
            vec![
                MutationIssue::Retryable(MutationRejection { publisher_seq: transient, reason: MutationRejectionReason::TimestampTooFarFuture }),
                MutationIssue::Rejected(MutationRejection { publisher_seq: first_permanent, reason: MutationRejectionReason::InvalidPayload }),
                MutationIssue::Rejected(MutationRejection { publisher_seq: second_permanent, reason: MutationRejectionReason::InvalidPayload }),
            ]
        );
        assert_eq!(validated.completed_sequences(), vec![accepted, first_permanent, second_permanent]);
    }

    fn cursor(lifecycle: u64, field: u64) -> SyncCursor {
        SyncCursor { lifecycle_revision: LibraryRevision::new(lifecycle).ok(), field_revision: LibraryRevision::new(field).ok() }
    }

    #[test]
    fn final_cursor_may_advance_past_the_last_returned_state() {
        let response = PullStateResponse { mutations: Vec::new(), next_cursor: cursor(10, 10), has_more: false };
        validate_pull_batch(&response, cursor(5, 0)).unwrap();
    }

    #[test]
    fn lifecycles_precede_fields_and_fields_stay_at_or_below_the_lifecycle_cursor() {
        let page = |mutations, next_cursor| PullStateResponse { mutations, next_cursor, has_more: false };
        validate_pull_batch(&page(vec![server_mutation("book_lifecycle", 30), server_mutation("metadata", 12)], cursor(30, 30)), cursor(10, 10)).unwrap();
        assert!(validate_pull_batch(&page(vec![server_mutation("metadata", 12), server_mutation("book_lifecycle", 30)], cursor(30, 30)), cursor(10, 10)).is_err(), "a field cannot precede a lifecycle");
        assert!(validate_pull_batch(&page(vec![server_mutation("metadata", 31)], cursor(30, 31)), cursor(30, 10)).is_err(), "a field cannot pass the lifecycle cursor");
        assert!(validate_pull_batch(&page(vec![server_mutation("metadata", 9)], cursor(30, 30)), cursor(30, 10)).is_err(), "a field must advance the field cursor");
    }

    #[test]
    fn a_non_final_page_ends_at_its_last_delivered_revision() {
        let lifecycles = PullStateResponse { mutations: vec![server_mutation("book_lifecycle", 22)], next_cursor: cursor(22, 10), has_more: true };
        validate_pull_batch(&lifecycles, cursor(20, 10)).unwrap();
        let skipped = PullStateResponse { next_cursor: cursor(40, 10), ..lifecycles.clone() };
        assert!(validate_pull_batch(&skipped, cursor(20, 10)).is_err(), "unread lifecycles cannot be skipped");
        let fields = PullStateResponse { mutations: vec![server_mutation("reading_position", 15)], next_cursor: cursor(40, 15), has_more: true };
        validate_pull_batch(&fields, cursor(40, 10)).unwrap();
    }
}
