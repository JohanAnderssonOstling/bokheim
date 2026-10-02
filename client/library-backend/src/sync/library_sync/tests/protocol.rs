use super::*;

#[test]
fn invalid_reading_positions_cannot_be_represented_on_the_wire() {
    assert!(book_model::ReadingPosition::parse("not-a-cfi").is_err());
    assert!(book_model::ReadingPosition::parse("epubcfi(/6/2)").is_ok());
    assert!(book_model::ReadingPosition::parse("pdfpage(3)").is_ok());
}

#[test]
fn pull_batch_requires_strict_sequences_after_the_cursor() {
    let valid = pull_response(vec![pulled_change(11, deleted(1)), pulled_change(12, deleted(2))], LibraryRevision::new(12).ok(), false);
    assert_eq!(validate_pull_batch(&valid, cursor(LibraryRevision::new(10).ok())), Ok(()));

    let repeated = pull_response(vec![pulled_change(11, deleted(1)), pulled_change(11, deleted(2))], LibraryRevision::new(11).ok(), false);
    assert!(validate_pull_batch(&repeated, cursor(LibraryRevision::new(10).ok())).is_err());
    assert!(validate_pull_batch(&valid, cursor(LibraryRevision::new(11).ok())).is_err());
}

#[test]
fn push_pages_are_bounded_by_count_and_preserve_sequence_order() {
    let mut changes = transmittable((1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 7).map(|sequence| reading_change(sequence, sequence, "epubcfi(/6/2)", sequence)).collect::<Vec<_>>());

    let first = changes.next_batch();
    let second = changes.next_batch();

    assert_eq!(first.len(), sync_common::MAX_PUSH_MUTATIONS);
    assert_eq!(second.len(), 7);
    assert_eq!(first.first().unwrap().publisher_seq.get(), 1);
    assert_eq!(second.first().unwrap().publisher_seq.get(), sync_common::MAX_PUSH_MUTATIONS as u64 + 1);
}

/// The service refuses a request larger than its decoded request limit, and
/// no batching makes an over-limit mutation fit. Left at the head of the
/// outbox it fails every exchange, so each retry blocks every later
/// mutation behind it. It must be set aside without being acknowledged:
/// dropping it would leave this replica silently diverged from the server.

#[test]
fn a_mutation_too_large_to_transmit_does_not_block_the_rest_of_the_outbox() {
    let ahead = reading_change(1, 1, "epubcfi(/6/2)", 1);
    let oversized = annotation_change(2, "x".repeat(sync_common::MAX_PUSH_BATCH_BYTES + 1));
    let behind = reading_change(3, 3, "epubcfi(/6/4)", 3);

    let mut queued = prepare_push(vec![ahead.clone(), oversized.clone(), behind.clone()]).unwrap();

    assert_eq!(queued.untransmittable(), &[oversized.publisher_seq], "only the untransmittable mutation is set aside");
    let page = queued.next_batch();
    assert_eq!(page.iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>(), vec![ahead.publisher_seq, behind.publisher_seq], "the mutations queued behind it still go out, in sequence order");
    assert!(queued.is_empty(), "the exchange loop has nothing left to spin on");
}

#[test]
fn push_pages_stop_before_the_encoded_byte_target() {
    let large_text = "x".repeat(64 * 1024);
    let mut queued = transmittable((1..=100_u64).map(|sequence| annotation_change(sequence, large_text.clone())).collect::<Vec<_>>());

    let page = queued.next_batch();
    // The budget is spent in transmitted bytes, so the page is measured in
    // the same wire encoding the request carries.
    let encoded_bytes = page.iter().map(|mutation| sync_common::transport::encode(mutation).unwrap().len()).sum::<usize>();

    assert!(!page.is_empty());
    assert!(page.len() < 100);
    assert!(encoded_bytes <= sync_common::MAX_PUSH_BATCH_BYTES);
    assert!(!queued.is_empty());
}

#[test]
fn push_coalesces_reading_positions_per_book_by_winning_version() {
    let changes = vec![
        reading_change(1, 1, "epubcfi(/6/10)", 10),
        (added(3, 1)).publication(sync_common::ReplicaId::nil(), millis(1), local_seq(2)),
        reading_change(3, 2, "epubcfi(/6/12)", 30),
        reading_change(4, 1, "epubcfi(/6/14)", 20),
        // Later local sequence, but an older wall clock: the server retains
        // sequence 3, so client compaction must make the same choice.
        reading_change(5, 2, "epubcfi(/6/16)", 20),
    ];

    let coalesced = coalesce_mutations_for_push(changes);
    assert_eq!(coalesced.iter().map(|change| change.publisher_seq.get()).collect::<Vec<_>>(), vec![2, 3, 4]);
    assert!(matches!(&coalesced[1].state.value, LibraryStateEntry::ReadingPosition { value: ReadingPositionState { location, .. }, .. } if location.as_str() == "epubcfi(/6/12)"));
    assert!(matches!(&coalesced[2].state.value, LibraryStateEntry::ReadingPosition { value: ReadingPositionState { location, .. }, .. } if location.as_str() == "epubcfi(/6/14)"));
}

#[test]
fn push_coalesces_every_protocol_singleton_but_preserves_append_events() {
    let dir_id = test_support::fixture_dir_id("singleton-dir");
    let changes = vec![
        LibraryStateEntry::DirectoryLifecycle { dir_id, value: library_replica::DirectoryLifecycleState::Deleted }.publication(sync_common::ReplicaId::nil(), millis(10), local_seq(1)),
        (added(2, 10)).publication(sync_common::ReplicaId::nil(), millis(10), local_seq(3)),
        (LibraryStateEntry::DirectoryLifecycle { dir_id, value: library_replica::DirectoryLifecycleState::Present }).publication(sync_common::ReplicaId::nil(), millis(10), local_seq(4)),
        reading_change(6, 1, "epubcfi(/6/18)", 10),
        reading_change(7, 1, "epubcfi(/6/20)", 20),
    ];

    let coalesced = coalesce_mutations_for_push(changes);
    assert_eq!(coalesced.iter().map(|change| change.publisher_seq.get()).collect::<Vec<_>>(), vec![1, 3, 7]);
    assert!(matches!(&coalesced[0].state.value, LibraryStateEntry::DirectoryLifecycle { value: library_replica::DirectoryLifecycleState::Deleted, .. }));
    assert!(matches!(&coalesced[1].state.value, LibraryStateEntry::BookLifecycle { .. }));
}

#[test]
fn coalesced_push_does_not_checkpoint_across_an_interleaved_rejection() {
    let changes = coalesce_mutations_for_push(vec![
        reading_change(1, 1, "epubcfi(/6/18)", 10),
        (added(2, 1)).publication(sync_common::ReplicaId::nil(), millis(1), local_seq(2)),
        reading_change(3, 1, "epubcfi(/6/22)", 20),
    ]);
    assert_eq!(changes.iter().map(|change| change.publisher_seq).collect::<Vec<_>>(), vec![local_seq(2), local_seq(3)]);

    let response = PushMutationsResponse { rejected: vec![MutationRejection { publisher_seq: changes[0].publisher_seq, reason: MutationRejectionReason::TimestampTooFarFuture }] };
    let requested_ids = changes.iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>();
    let validated = validate_push_response(&requested_ids, &response).unwrap();
    assert_eq!(validated.accepted, vec![changes[1].publisher_seq]);
    assert_eq!(validated.issues, vec![sync_common::MutationIssue::Retryable(MutationRejection { publisher_seq: changes[0].publisher_seq, reason: MutationRejectionReason::TimestampTooFarFuture })]);
}

#[test]
fn push_acceptance_uses_mutation_identity() {
    let requested = [reading_change(1, 1, "epubcfi(/6/24)", 1), reading_change(2, 2, "epubcfi(/6/26)", 2)];
    let response = PushMutationsResponse { rejected: Vec::new() };
    let requested_ids = requested.iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>();
    assert_eq!(validate_push_response(&requested_ids, &response).unwrap().accepted, requested_ids);
}

#[test]
fn permanent_rejection_is_reported_by_mutation_identity() {
    let requested = [reading_change(1, 1, "epubcfi(/6/24)", 1)];
    let response = PushMutationsResponse { rejected: vec![MutationRejection { publisher_seq: requested[0].publisher_seq, reason: MutationRejectionReason::InvalidPayload }] };
    let requested_ids = requested.iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>();
    assert_eq!(validate_push_response(&requested_ids, &response).unwrap().issues, vec![sync_common::MutationIssue::Rejected(MutationRejection { publisher_seq: requested[0].publisher_seq, reason: MutationRejectionReason::InvalidPayload })]);
}

#[test]
fn foreign_or_duplicate_rejections_are_rejected() {
    let requested = [reading_change(1, 1, "epubcfi(/6/24)", 1), reading_change(2, 2, "epubcfi(/6/26)", 2)];
    let requested_ids = requested.iter().map(|mutation| mutation.publisher_seq).collect::<Vec<_>>();
    let missing = PushMutationsResponse { rejected: vec![MutationRejection { publisher_seq: sync_common::ReplicaSeq::new(99).unwrap(), reason: MutationRejectionReason::InvalidPayload }] };
    assert!(matches!(validate_push_response(&requested_ids, &missing), Err(PushResponseError::InvalidAcknowledgments(_))));

    let contradictory = PushMutationsResponse { rejected: vec![MutationRejection { publisher_seq: requested[0].publisher_seq, reason: MutationRejectionReason::TimestampTooFarFuture }] };
    let contradictory = PushMutationsResponse { rejected: vec![contradictory.rejected[0].clone(), contradictory.rejected[0].clone()] };
    assert!(matches!(validate_push_response(&requested_ids[..1], &contradictory), Err(PushResponseError::InvalidAcknowledgments(_))));
}
