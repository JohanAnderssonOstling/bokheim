-- Legacy permanent identities must be migrated/reimported before dropping
-- their byte-hash mapping; never silently serve different bytes under a key.
DO $$
BEGIN
    IF EXISTS(SELECT 1 FROM user_book_revision WHERE content_hash<>blob_hash)
       OR EXISTS(SELECT 1 FROM library_blob_claim WHERE content_hash<>blob_hash) THEN
        RAISE EXCEPTION 'Legacy book identities differ from byte hashes; migrate or reimport these books before upgrading to schema 5';
    END IF;
END $$;
DROP TABLE user_book_revision;
DROP INDEX library_blob_claim_user_blob;
ALTER TABLE library_blob_claim DROP COLUMN blob_hash;
ALTER TABLE library_blob_claim ADD FOREIGN KEY(content_hash) REFERENCES blob_object(content_hash);
CREATE INDEX library_blob_claim_user_hash ON library_blob_claim(user_id,content_hash);
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=5;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=5);
