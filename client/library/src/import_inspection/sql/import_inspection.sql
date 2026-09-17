-- Durable proof that a local import was successfully inspected.

-- name: has_import_inspection ->
-- param: content_hash: &str
-- param: format: &str
-- param: version: i64
SELECT EXISTS(
    SELECT 1 FROM local_import_inspection i JOIN book b USING(content_hash)
    WHERE i.content_hash = :content_hash AND i.format = :format AND i.version = :version
      AND b.format = :format AND b.deleted_at IS NULL
);

-- name: record_import_inspection!
-- param: content_hash: &str
-- param: format: &str
-- param: version: i64
INSERT INTO local_import_inspection(content_hash, format, version)
VALUES (:content_hash, :format, :version)
ON CONFLICT(content_hash) DO UPDATE SET format = excluded.format, version = excluded.version;
