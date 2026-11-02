-- Keep portable book IDs separate from the immutable uploaded byte digest.
ALTER TABLE blob_object ADD COLUMN byte_checksum bytea;
UPDATE blob_object SET byte_checksum=content_hash;
ALTER TABLE blob_object ALTER COLUMN byte_checksum SET NOT NULL;
ALTER TABLE blob_object ADD CHECK(octet_length(byte_checksum)=32);
CREATE TABLE canonical_blob_object (
    byte_checksum bytea PRIMARY KEY CHECK(octet_length(byte_checksum)=32),
    size_bytes bigint NOT NULL CHECK(size_bytes>0)
);
INSERT INTO canonical_blob_object(byte_checksum,size_bytes) SELECT byte_checksum,size_bytes FROM blob_object;
ALTER TABLE blob_object ADD FOREIGN KEY(byte_checksum) REFERENCES canonical_blob_object(byte_checksum) ON DELETE RESTRICT;
CREATE INDEX blob_object_checksum ON blob_object(byte_checksum,content_hash);
CREATE TABLE user_canonical_charge (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    byte_checksum bytea NOT NULL REFERENCES canonical_blob_object(byte_checksum) ON DELETE RESTRICT,
    charged_at timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY(user_id,byte_checksum)
);
CREATE INDEX user_canonical_charge_checksum ON user_canonical_charge(byte_checksum);
INSERT INTO user_canonical_charge(user_id,byte_checksum,charged_at)
SELECT charge.user_id,object.byte_checksum,MIN(charge.charged_at)
FROM user_blob_charge charge JOIN blob_object object USING(content_hash)
GROUP BY charge.user_id,object.byte_checksum;
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=14;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=14);
