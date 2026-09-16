SELECT
 (SELECT count(*) FROM book WHERE deleted_at IS NULL AND hidden_at IS NULL),
 (SELECT count(*) FROM remote_asset WHERE kind='book' AND hash IN (SELECT content_hash FROM book WHERE deleted_at IS NULL AND hidden_at IS NULL)),
 (SELECT count(*) FROM local_book_upload),
 (SELECT count(*) FROM sync_outbox),
 (SELECT count(*) FROM local_thumbnail_work WHERE state='pending'),
 (SELECT count(*) FROM local_thumbnail_work t JOIN local_thumbnail_source s USING(content_hash)
  WHERE t.state='ready' AND s.origin='local' AND NOT EXISTS(SELECT 1 FROM remote_asset r WHERE r.kind='thumbnail' AND r.hash=t.content_hash)),
 (SELECT count(*) FROM rejected_asset_upload r WHERE r.rejected_at>unixepoch()-300 AND
  ((r.kind='book' AND EXISTS(SELECT 1 FROM local_book_upload u WHERE u.content_hash=r.hash)) OR
   (r.kind='thumbnail' AND EXISTS(SELECT 1 FROM local_thumbnail_work t JOIN local_thumbnail_source s USING(content_hash) WHERE t.content_hash=r.hash AND t.state='ready' AND s.origin='local') AND NOT EXISTS(SELECT 1 FROM remote_asset a WHERE a.kind='thumbnail' AND a.hash=r.hash)))),
 EXISTS(SELECT 1 FROM sync_metadata WHERE key='last_successful_sync_at')
