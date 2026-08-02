use std::collections::{HashMap, HashSet};
use sync_common::{LocalChange, LocalSeq, PushChangesResponse, PushRejectionReason, ServerChange, ServerSeq};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PushResponseError {
    InvalidAcknowledgments(String),
    PermanentRejection { local_seq: LocalSeq, reason: PushRejectionReason },
}

impl std::fmt::Display for PushResponseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAcknowledgments(reason) => write!(formatter, "invalid push acknowledgment response: {reason}"),
            Self::PermanentRejection { local_seq, reason } => write!(formatter, "server permanently rejected local sync event {local_seq}: {reason:?}"),
        }
    }
}

impl std::error::Error for PushResponseError {}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PullBatchError(String);

impl std::fmt::Display for PullBatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid pull batch: {}", self.0)
    }
}

impl std::error::Error for PullBatchError {}

pub(super) fn validate_pull_batch(changes: &[ServerChange], since_server_seq: Option<ServerSeq>) -> Result<(), PullBatchError> {
    let mut previous_server_seq = since_server_seq.map(ServerSeq::get).unwrap_or(0);
    for change in changes {
        let server_seq = change.server_seq.get();
        if server_seq <= previous_server_seq {
            return Err(PullBatchError(format!("server sequence {server_seq} does not follow {previous_server_seq}")));
        }
        previous_server_seq = server_seq;
        for (name, value) in [("changed_at", change.changed_at), ("hlc.pt", change.hlc.pt), ("hlc.l", change.hlc.l)] {
            if value > i64::MAX as u64 {
                return Err(PullBatchError(format!("{name} exceeds the database integer range at server sequence {server_seq}")));
            }
        }
        change.op.validate().map_err(|error| PullBatchError(format!("{error} at server sequence {server_seq}")))?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ValidatedPushResponse {
    pub(super) checkpoint: Option<LocalSeq>,
    pub(super) permanent_rejection: Option<(LocalSeq, PushRejectionReason)>,
}

/// Validates that the server accounted for every submitted event exactly once.
/// Only the leading run of handled events is safe to checkpoint; any rejection
/// is a barrier even if later events were independently accepted.
pub(super) fn validate_push_response(requested: &[LocalSeq], response: &PushChangesResponse) -> Result<ValidatedPushResponse, PushResponseError> {
    let requested_set: HashSet<_> = requested.iter().copied().collect();
    if requested_set.len() != requested.len() || requested.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(PushResponseError::InvalidAcknowledgments("submitted local sequences are not strictly increasing and unique".to_string()));
    }

    let mut handled = HashSet::new();
    for sequence in &response.handled_local_seqs {
        if !requested_set.contains(sequence) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server handled unsubmitted local sequence {sequence}")));
        }
        if !handled.insert(*sequence) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server handled local sequence {sequence} more than once")));
        }
    }

    let mut rejected = HashMap::new();
    for rejection in &response.rejected {
        if !requested_set.contains(&rejection.local_seq) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server rejected unsubmitted local sequence {}", rejection.local_seq)));
        }
        if handled.contains(&rejection.local_seq) {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server both handled and rejected local sequence {}", rejection.local_seq)));
        }
        if rejected.insert(rejection.local_seq, rejection.reason).is_some() {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server rejected local sequence {} more than once", rejection.local_seq)));
        }
    }

    let mut checkpoint = None;
    let mut hit_barrier = false;
    for sequence in requested {
        if handled.contains(sequence) {
            if !hit_barrier {
                checkpoint = Some(*sequence);
            }
        } else if rejected.contains_key(sequence) {
            hit_barrier = true;
        } else {
            return Err(PushResponseError::InvalidAcknowledgments(format!("server omitted local sequence {sequence}")));
        }
    }

    let permanent_rejection = requested.iter().find_map(|sequence| rejected.get(sequence).copied().filter(|reason| !reason.is_transient()).map(|reason| (*sequence, reason)));
    Ok(ValidatedPushResponse { checkpoint, permanent_rejection })
}

/// Keep only the greatest unsent version of each singleton entity. Older values
/// are already dominated by that representative and need not cross the network.
/// Append/lifecycle event types retain their original order and multiplicity.
pub(super) fn coalesce_singletons_for_push(changes: Vec<LocalChange>) -> Vec<LocalChange> {
    let mut winners: HashMap<(&'static str, String), (u64, u64, u8, LocalSeq)> = HashMap::new();
    for change in &changes {
        if !change.op.is_singleton() {
            continue;
        }
        let key = (change.op.event_type(), change.op.entity_id().into_owned());
        let version = (change.hlc.pt, change.hlc.l, change.op.conflict_rank(), change.local_seq);
        winners.entry(key).and_modify(|winner| *winner = (*winner).max(version)).or_insert(version);
    }

    changes
        .into_iter()
        .filter(|change| {
            if !change.op.is_singleton() {
                return true;
            }
            let key = (change.op.event_type(), change.op.entity_id().into_owned());
            winners.get(&key).is_some_and(|winner| *winner == (change.hlc.pt, change.hlc.l, change.op.conflict_rank(), change.local_seq))
        })
        .collect()
}
