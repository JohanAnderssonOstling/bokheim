ALTER TABLE libraries ADD COLUMN IF NOT EXISTS cloud_storage_enabled boolean NOT NULL DEFAULT true;
CREATE TABLE IF NOT EXISTS library_blob_claim (
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content_hash text NOT NULL,
    blob_hash text NOT NULL REFERENCES blob_object(content_hash),
    PRIMARY KEY(library_id, content_hash)
);
CREATE INDEX IF NOT EXISTS library_blob_claim_user_blob ON library_blob_claim(user_id, blob_hash);
INSERT INTO library_blob_claim(library_id,user_id,content_hash,blob_hash)
SELECT DISTINCT state.library_id,library.user_id,state.content_hash,COALESCE(revision.blob_hash,state.content_hash)
FROM sync_state state JOIN libraries library ON library.id=state.library_id AND library.deleted_at IS NULL
LEFT JOIN user_book_revision revision ON revision.user_id=library.user_id AND revision.content_hash=state.content_hash
JOIN user_blob_charge charge ON charge.user_id=library.user_id AND charge.content_hash=COALESCE(revision.blob_hash,state.content_hash)
WHERE state.kind='book_lifecycle' AND state.present AND state.content_hash IS NOT NULL
ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS cloud_storage_change (
 library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 change_id uuid NOT NULL,
 PRIMARY KEY(library_id,change_id)
);
