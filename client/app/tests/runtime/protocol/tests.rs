use super::*;
use library_backend::BookId;

#[tokio::test]
async fn library_list_watch_delivers_coalesced_wire_notifications_and_closes() {
    let (sender, receiver) = watch::channel(());
    let mut events = WorkerSubscription::LibraryList(receiver);
    sender.send_replace(());
    sender.send_replace(());
    let payload = events.next_value().await.unwrap();
    client_runtime::wire::decode_worker_payload::<()>(payload).unwrap();
    drop(sender);
    assert!(events.next_value().await.is_none(), "coalesced notifications should be consumed before closure");
}

#[test]
fn rejects_flat_book_subscription_requests() {
    let request = serde_json::json!({
        "Request": {
            "id": 1,
            "command": {
                "App": {
                    "SubscribeBook": {
                        "library_id": LibraryId::new_v4(),
                        "content_hash": BookId::from_bytes([7; 32]),
                    }
                }
            }
        }
    });
    let bytes = encode_worker_message(&request).unwrap();
    let error = decode_worker_message::<AppWorkerClientMessage>(&bytes).unwrap_err();
    assert!(error.to_string().contains("unknown variant `SubscribeBook`"), "{error}");
}

#[test]
fn accepts_library_scoped_subscriptions() {
    let library_id = LibraryId::new_v4();
    let request = AppWorkerClientMessage::Request(AppWorkerRequest { id: 1, command: WorkerCommand::LibrarySubscription { library_id, subscription: LibrarySubscription::Updates } });
    let bytes = encode_worker_message(&request).unwrap();
    let decoded = decode_worker_message::<AppWorkerClientMessage>(&bytes).unwrap();
    assert!(matches!(decoded, AppWorkerClientMessage::Request(AppWorkerRequest { id: 1, command: WorkerCommand::LibrarySubscription { library_id: id, subscription: LibrarySubscription::Updates } }) if id == library_id));
}
