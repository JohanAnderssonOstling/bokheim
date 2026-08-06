use crate::extract::{sync_error, AuthUser};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use sync_common::api::libraries::UpsertLibraryRequest;
use sync_common::api::notifications::NotificationMessage;
use sync_common::{MutationBody, MutationId, PullStateQuery, PushMutationsRequest, SyncExchangeRequest};

pub(super) async fn list_libraries(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, axum::http::StatusCode> {
    let libraries = state.sync.list_libraries(&user.user_id).await.map_err(sync_error)?;
    crate::api_wire::response(&headers, libraries)
}

pub(super) async fn upsert_library(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: UpsertLibraryRequest = crate::api_wire::decode_request(&headers, &body)?;
    let outcome = state.sync.upsert_library(&user.user_id, request).await.map_err(sync_error)?;
    if outcome.changed {
        state.notifications.publish(&user.user_id, Some(user.credential), NotificationMessage::CatalogueChanged);
    }
    crate::api_wire::response(&headers, outcome.library)
}

pub(super) async fn push_events(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: PushMutationsRequest = crate::api_wire::decode_request(&headers, &body)?;
    let candidates = notification_candidates(&request.library_id, &request.mutations);
    let response = state.sync.push(&user.user_id, request).await.map_err(sync_error)?;
    publish_accepted(&state, &user, candidates, &response.accepted);
    crate::api_wire::response(&headers, response)
}

pub(super) async fn pull_events(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, Query(query): Query<PullStateQuery>) -> Result<Response, axum::http::StatusCode> {
    let response = state.sync.pull(&user.user_id, query).await.map_err(sync_error)?;
    crate::api_wire::response(&headers, response)
}

pub(super) async fn exchange_events(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: SyncExchangeRequest = crate::api_wire::decode_request(&headers, &body)?;
    let candidates = notification_candidates(&request.library_id, &request.mutations);
    let response = state.sync.exchange(&user.user_id, request).await.map_err(sync_error)?;
    publish_accepted(&state, &user, candidates, &response.push.accepted);
    crate::api_wire::response(&headers, response)
}

#[derive(Clone, Copy)]
struct CandidateKind {
    mutation_id: MutationId,
    reading: bool,
    other: bool,
    assets: bool,
}

fn notification_candidates(library_id: &sync_common::LibraryId, mutations: &[sync_common::StateMutation]) -> (sync_common::LibraryId, Vec<CandidateKind>) {
    let candidates = mutations
        .iter()
        .map(|mutation| CandidateKind {
            mutation_id: mutation.mutation_id,
            reading: matches!(mutation.body, MutationBody::ReadingPosition { .. }),
            other: !matches!(mutation.body, MutationBody::ReadingPosition { .. }),
            assets: matches!(mutation.body, MutationBody::BookLifecycle { .. } | MutationBody::Placement { .. }),
        })
        .collect();
    (library_id.clone(), candidates)
}

fn publish_accepted(state: &AppState, user: &AuthUser, (library_id, candidates): (sync_common::LibraryId, Vec<CandidateKind>), accepted: &[MutationId]) {
    let Some(message) = accepted_notification(library_id, &candidates, accepted) else { return };
    state.notifications.publish(&user.user_id, Some(user.credential.clone()), message);
}

fn accepted_notification(library_id: sync_common::LibraryId, candidates: &[CandidateKind], accepted: &[MutationId]) -> Option<NotificationMessage> {
    let mut reading = false;
    let mut other = false;
    let mut assets = false;
    for candidate in candidates.iter().filter(|candidate| accepted.contains(&candidate.mutation_id)) {
        reading |= candidate.reading;
        other |= candidate.other;
        assets |= candidate.assets;
    }
    (reading || other).then_some(NotificationMessage::LibraryChanged { library_id, reading, other, assets })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync_common::{BookId, BookLifecycleState, LibraryId, ReadingPositionState, ReplicaSeq, StateMutation, UnixMillis};

    #[test]
    fn candidates_distinguish_reading_from_asset_affecting_state() {
        let reading_id = MutationId::new();
        let mutation = StateMutation {
            mutation_id: reading_id,
            body: MutationBody::ReadingPosition { book_id: BookId::new(1), value: ReadingPositionState { location: "epubcfi(/6/2)".to_owned(), progress: 1.0 } },
            changed_at: UnixMillis::new(1).unwrap(),
            replica_seq: ReplicaSeq::new(1).unwrap(),
        };
        let lifecycle = StateMutation {
            mutation_id: MutationId::new(),
            body: MutationBody::BookLifecycle {
                book_id: BookId::new(2),
                value: BookLifecycleState::Present {
                    metadata: sync_common::BookMetadataState {
                        title: "Book".to_owned(),
                        authors: Vec::new(),
                        description: String::new(),
                        publication: Default::default(),
                    },
                    added_at: UnixMillis::new(1).unwrap(),
                },
            },
            changed_at: UnixMillis::new(2).unwrap(),
            replica_seq: ReplicaSeq::new(2).unwrap(),
        };
        let (_, candidates) = notification_candidates(&LibraryId::parse("library").unwrap(), &[mutation, lifecycle]);
        assert_eq!(candidates.len(), 2);
        assert!(candidates[0].reading);
        assert!(!candidates[0].other);
        assert!(!candidates[0].assets);
        assert!(!candidates[1].reading);
        assert!(candidates[1].other);
        assert!(candidates[1].assets);
    }

    #[test]
    fn notification_announces_only_accepted_mutation_channels() {
        let reading_id = MutationId::new();
        let lifecycle_id = MutationId::new();
        let library_id = LibraryId::parse("notification-library").unwrap();
        let candidates = vec![
            CandidateKind { mutation_id: reading_id, reading: true, other: false, assets: false },
            CandidateKind { mutation_id: lifecycle_id, reading: false, other: true, assets: true },
        ];
        assert_eq!(
            accepted_notification(library_id.clone(), &candidates, &[reading_id]),
            Some(NotificationMessage::LibraryChanged { library_id: library_id.clone(), reading: true, other: false, assets: false }),
        );
        assert_eq!(accepted_notification(library_id, &candidates, &[]), None);
    }
}
