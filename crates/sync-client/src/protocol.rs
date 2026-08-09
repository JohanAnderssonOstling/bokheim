use std::collections::HashSet;
use domain::{reduce_same_replica_mutations, StateMutation};
use sync_common::{LibraryRevision, MutationId, MutationRejectionReason, PullStateResponse, PushMutationsResponse};

#[derive(Debug, PartialEq, Eq)]
pub enum PushResponseError {
    InvalidAcknowledgments(String),
    TransientRejection { mutation_ids: Vec<MutationId> },
    PermanentRejection { mutation_id: MutationId, reason: MutationRejectionReason },
}

impl std::fmt::Display for PushResponseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAcknowledgments(reason) => write!(formatter, "invalid mutation acknowledgement response: {reason}"),
            Self::TransientRejection { mutation_ids } => write!(formatter, "server deferred {} synchronization mutation(s)", mutation_ids.len()),
            Self::PermanentRejection { mutation_id, reason } => write!(formatter, "server permanently rejected mutation {mutation_id}: {reason:?}"),
        }
    }
}

impl std::error::Error for PushResponseError {}

#[derive(Debug, PartialEq, Eq)]
pub struct PullBatchError(String);

impl std::fmt::Display for PullBatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid pull batch: {}", self.0)
    }
}

impl std::error::Error for PullBatchError {}

pub fn validate_pull_batch(response: &PullStateResponse, cursor: Option<LibraryRevision>) -> Result<(), PullBatchError> {
    let starting_revision = cursor.map(LibraryRevision::get).unwrap_or(0);
    let mut previous_revision = starting_revision;
    for mutation in &response.mutations {
        let revision = mutation.revision.get();
        if revision <= previous_revision {
            return Err(PullBatchError(format!("library revision {revision} does not follow {previous_revision}")));
        }
        previous_revision = revision;
    }
    let next = response.next_cursor.map(LibraryRevision::get).unwrap_or(0);
    if next < previous_revision || next < starting_revision {
        return Err(PullBatchError(format!("next cursor {next} precedes observed revision {previous_revision}")));
    }
    if response.has_more && response.mutations.is_empty() {
        return Err(PullBatchError("a non-final page cannot be empty".into()));
    }
    if response.has_more && next != previous_revision {
        return Err(PullBatchError("a non-final page cursor must equal its final mutation revision".into()));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedPushResponse {
    pub accepted: Vec<MutationId>,
    pub transient_rejections: Vec<MutationId>,
    pub permanent_rejections: Vec<(MutationId, MutationRejectionReason)>,
}

pub fn validate_push_response(requested: &[StateMutation], response: &PushMutationsResponse) -> Result<ValidatedPushResponse, PushResponseError> {
    let requested_ids: HashSet<_> = requested.iter().map(|mutation| mutation.mutation_id).collect();
    if requested_ids.len() != requested.len() {
        return Err(PushResponseError::InvalidAcknowledgments("submitted mutation identities are not unique".into()));
    }
    let mut accounted = HashSet::new();
    for mutation_id in &response.accepted {
        if !requested_ids.contains(mutation_id) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server accepted unsubmitted mutation {mutation_id}")));
        }
        if !accounted.insert(*mutation_id) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server accounted for mutation {mutation_id} more than once")));
        }
    }
    let mut transient_rejections = Vec::new();
    let mut permanent_rejections = Vec::new();
    for rejection in &response.rejected {
        if !requested_ids.contains(&rejection.mutation_id) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server rejected unsubmitted mutation {}", rejection.mutation_id)));
        }
        if !accounted.insert(rejection.mutation_id) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server accounted for mutation {} more than once", rejection.mutation_id)));
        }
        if rejection.reason.is_transient() {
            transient_rejections.push(rejection.mutation_id);
        } else {
            permanent_rejections.push((rejection.mutation_id, rejection.reason));
        }
    }
    if accounted != requested_ids {
        return Err(PushResponseError::InvalidAcknowledgments("server omitted one or more submitted mutations".into()));
    }
    Ok(ValidatedPushResponse { accepted: response.accepted.clone(), transient_rejections, permanent_rejections })
}

/// Reduces unsent local mutations to one canonical winner per state cell.
/// Placement releases are conditional: releasing displaced book A never
/// removes a later local claim by book B.
pub fn coalesce_mutations_for_push(mutations: Vec<StateMutation>) -> Vec<StateMutation> {
    reduce_same_replica_mutations(&mutations).into_iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{MutationBody, ReadingPositionState};
    use sync_common::{ContentHash, ClientId, ReplicaSeq};

    fn mutation(sequence: u64, body: MutationBody) -> StateMutation {
        StateMutation { mutation_id: MutationId::new(), body, changed_at: sequence, replica_seq: ReplicaSeq::new(sequence).unwrap() }
    }

    #[test]
    fn identity_acknowledgements_do_not_depend_on_sequence_prefixes() {
        let first = mutation(1, MutationBody::ReadingPosition { content_hash: ContentHash::from_fixture_u64(1), value: ReadingPositionState { location: sync_common::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 1.0 } });
        let second = mutation(2, MutationBody::ReadingPosition { content_hash: ContentHash::from_fixture_u64(2), value: ReadingPositionState { location: sync_common::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 1.0 } });
        let response = PushMutationsResponse { accepted: vec![second.mutation_id, first.mutation_id], rejected: Vec::new() };
        let validated = validate_push_response(&[first.clone(), second.clone()], &response).unwrap();
        assert_eq!(validated.accepted, vec![second.mutation_id, first.mutation_id]);
    }

    #[test]
    fn every_requested_identity_is_partitioned_by_rejection_lifetime() {
        let position = |content_hash| MutationBody::ReadingPosition { content_hash: ContentHash::from_fixture_u64(content_hash), value: ReadingPositionState { location: sync_common::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 1.0 } };
        let accepted = mutation(1, position(1));
        let transient = mutation(2, position(2));
        let first_permanent = mutation(3, position(3));
        let second_permanent = mutation(4, position(4));
        let response = PushMutationsResponse {
            accepted: vec![accepted.mutation_id],
            rejected: vec![
                sync_common::MutationRejection { mutation_id: transient.mutation_id, reason: MutationRejectionReason::MissingDependency },
                sync_common::MutationRejection { mutation_id: first_permanent.mutation_id, reason: MutationRejectionReason::InvalidPayload },
                sync_common::MutationRejection { mutation_id: second_permanent.mutation_id, reason: MutationRejectionReason::InvalidPayload },
            ],
        };

        let validated = validate_push_response(&[accepted.clone(), transient.clone(), first_permanent.clone(), second_permanent.clone()], &response).unwrap();
        assert_eq!(validated.accepted, vec![accepted.mutation_id]);
        assert_eq!(validated.transient_rejections, vec![transient.mutation_id]);
        assert_eq!(validated.permanent_rejections, vec![(first_permanent.mutation_id, MutationRejectionReason::InvalidPayload), (second_permanent.mutation_id, MutationRejectionReason::InvalidPayload),]);
    }

    #[test]
    fn placement_updates_coalesce_to_the_latest_register_value() {
        let dir_id = sync_common::DirId::parse("shelf").unwrap();
        let claim = mutation(1, MutationBody::Placement { dir_id: dir_id.clone(), content_hash: ContentHash::from_fixture_u64(2), present: true });
        let stale_release = mutation(2, MutationBody::Placement { dir_id, content_hash: ContentHash::from_fixture_u64(2), present: false });
        let reduced = coalesce_mutations_for_push(vec![claim.clone(), stale_release]);
        assert_eq!(reduced.len(), 1);
        assert!(matches!(reduced[0].body, MutationBody::Placement { present: false, .. }));
    }

    #[test]
    fn final_cursor_may_advance_past_the_last_returned_state() {
        let response = PullStateResponse { mutations: Vec::new(), next_cursor: LibraryRevision::new(10).ok(), has_more: false };
        validate_pull_batch(&response, LibraryRevision::new(5).ok()).unwrap();
        let _ = ClientId::parse("replica").unwrap();
    }
}
