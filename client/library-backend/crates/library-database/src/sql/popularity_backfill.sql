-- Read one immutable addition at a time during the schema upgrade.
-- Unanswered local drafts have no popularity and retain the column's NULL.
SELECT record.content_hash,record.body
FROM sync_book_record record JOIN book USING(content_hash)
WHERE record.state_kind='book_add';
