-- Immutable bytes stay keyed by canonical checksum. Only logical book IDs move.
INSERT INTO blob_object(content_hash,byte_checksum,size_bytes,created_at)
SELECT m.new_id,b.byte_checksum,b.size_bytes,min(b.created_at)
FROM blob_object b JOIN book_id_rekey m ON m.old_id=b.content_hash
GROUP BY m.new_id,b.byte_checksum,b.size_bytes ON CONFLICT DO NOTHING;
INSERT INTO library_blob_claim(library_id,user_id,content_hash)
SELECT c.library_id,c.user_id,m.new_id FROM library_blob_claim c JOIN book_id_rekey m ON m.old_id=c.content_hash ON CONFLICT DO NOTHING;
DELETE FROM library_blob_claim c USING book_id_rekey m WHERE c.content_hash=m.old_id AND m.old_id<>m.new_id;
INSERT INTO user_blob_charge(user_id,content_hash,charged_at)
SELECT c.user_id,m.new_id,min(c.charged_at) FROM user_blob_charge c JOIN book_id_rekey m ON m.old_id=c.content_hash GROUP BY c.user_id,m.new_id ON CONFLICT DO NOTHING;
DELETE FROM user_blob_charge c USING book_id_rekey m WHERE c.content_hash=m.old_id AND m.old_id<>m.new_id;
-- Accepted lifecycles, including trash, are the source of truth for references.
DELETE FROM sync_state;
INSERT INTO sync_state SELECT * FROM rekeyed_state;
DELETE FROM user_blob_reference;
INSERT INTO user_blob_reference(user_id,content_hash,reference_count)
SELECT l.user_id,s.content_hash,count(*) FROM sync_state s JOIN libraries l ON l.id=s.library_id
WHERE s.kind='book_lifecycle' AND s.version_rank<>2 AND s.content_hash IS NOT NULL GROUP BY l.user_id,s.content_hash;
DELETE FROM blob_object b USING book_id_rekey m WHERE b.content_hash=m.old_id AND m.old_id<>m.new_id;
-- Old clients cannot finish their outstanding scoped upload reservations.
WITH removed AS (DELETE FROM book_upload_reservation WHERE octet_length(content_hash)=33 AND get_byte(content_hash,0)=2 RETURNING user_id,size_bytes),
released AS (SELECT user_id,sum(size_bytes)::bigint bytes FROM removed GROUP BY user_id)
UPDATE user_storage_account a SET reserved_bytes=a.reserved_bytes-r.bytes FROM released r WHERE a.user_id=r.user_id;
UPDATE sync_history SET generation=gen_random_uuid() WHERE singleton=true;
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=20;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=20);
