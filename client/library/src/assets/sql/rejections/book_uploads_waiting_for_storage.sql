SELECT EXISTS (
    SELECT 1 FROM rejected_asset_upload rejected
    JOIN local_book_upload upload ON upload.content_hash = rejected.hash
    WHERE rejected.kind = 'book'
      AND rejected.reason = 'server rejected upload admission: QuotaExceeded'
      AND rejected.rejected_at > unixepoch() - 300
)
