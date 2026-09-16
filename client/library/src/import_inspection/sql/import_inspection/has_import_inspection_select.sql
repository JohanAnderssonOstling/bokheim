SELECT EXISTS(SELECT 1 FROM local_import_inspection i JOIN book b USING(content_hash)
         WHERE i.content_hash=?1 AND i.format=?2 AND i.version=?3 AND b.format=?2 AND b.deleted_at IS NULL)