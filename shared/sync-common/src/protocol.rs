//! Transport-independent validation for synchronization pages.

use crate::{LibraryRevision, MutationId, MutationRejectionReason, PullStateResponse, PushMutationsResponse};
use std::collections::HashSet;

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

pub fn validate_push_response(requested: &[MutationId], response: &PushMutationsResponse) -> Result<ValidatedPushResponse, PushResponseError> {
    let requested_ids: HashSet<_> = requested.iter().copied().collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_acknowledgements_do_not_depend_on_sequence_prefixes() {
        let first = MutationId::new();
        let second = MutationId::new();
        let response = PushMutationsResponse { accepted: vec![second, first], rejected: Vec::new() };
        let validated = validate_push_response(&[first, second], &response).unwrap();
        assert_eq!(validated.accepted, vec![second, first]);
    }

    #[test]
    fn every_requested_identity_is_partitioned_by_rejection_lifetime() {
        let accepted = MutationId::new();
        let transient = MutationId::new();
        let first_permanent = MutationId::new();
        let second_permanent = MutationId::new();
        let response = PushMutationsResponse {
            accepted: vec![accepted],
            rejected: vec![
                crate::MutationRejection { mutation_id: transient, reason: MutationRejectionReason::MissingDependency },
                crate::MutationRejection { mutation_id: first_permanent, reason: MutationRejectionReason::InvalidPayload },
                crate::MutationRejection { mutation_id: second_permanent, reason: MutationRejectionReason::InvalidPayload },
            ],
        };

        let validated = validate_push_response(&[accepted, transient, first_permanent, second_permanent], &response).unwrap();
        assert_eq!(validated.accepted, vec![accepted]);
        assert_eq!(validated.transient_rejections, vec![transient]);
        assert_eq!(validated.permanent_rejections, vec![(first_permanent, MutationRejectionReason::InvalidPayload), (second_permanent, MutationRejectionReason::InvalidPayload),]);
    }

    #[test]
    fn final_cursor_may_advance_past_the_last_returned_state() {
        let response = PullStateResponse { mutations: Vec::new(), next_cursor: LibraryRevision::new(10).ok(), has_more: false };
        validate_pull_batch(&response, LibraryRevision::new(5).ok()).unwrap();
        let _ = "replica".to_owned();
    }
}
