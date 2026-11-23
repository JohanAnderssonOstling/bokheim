-- Lookup and foreign-key checks used by collection and account deletion.
CREATE INDEX libraries_user ON libraries(user_id);
CREATE INDEX user_blob_charge_hash ON user_blob_charge(content_hash);
CREATE INDEX library_blob_claim_hash ON library_blob_claim(content_hash);
CREATE INDEX sync_state_library_hash ON sync_state(library_id, content_hash);
CREATE INDEX sync_state_lifecycle_hash ON sync_state(content_hash, library_id)
    WHERE kind='book_lifecycle' AND version_rank<>2 AND content_hash IS NOT NULL;
CREATE INDEX auth_security_event_time ON auth_security_event(detected_at);

-- Repair any reservation-ledger drift left by the old library deletion path.
-- The upgrade runs before traffic; table locks also exclude older writers.
LOCK TABLE user_storage_account, book_upload_reservation IN SHARE ROW EXCLUSIVE MODE;
UPDATE user_storage_account account
SET reserved_bytes=COALESCE((SELECT SUM(size_bytes) FROM book_upload_reservation reservation WHERE reservation.user_id=account.user_id), 0)
WHERE reserved_bytes IS DISTINCT FROM COALESCE((SELECT SUM(size_bytes) FROM book_upload_reservation reservation WHERE reservation.user_id=account.user_id), 0);
