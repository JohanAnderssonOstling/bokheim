-- Stable author identities and ordered book credits/contributors.

-- name: existing_book_contributor_credit?
-- param: content_hash: &str
-- param: position: i64
SELECT author_identity.id, author_identity.stable_id, book_contributor.name
FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND book_contributor.position=:position;

-- name: author_identity_by_stable_id?
-- param: stable_id: &str
SELECT id, stable_id FROM author_identity WHERE stable_id=:stable_id;

-- name: provisional_author_identity_by_name?
-- param: normalized_name: &str
SELECT id, stable_id
FROM author_identity
WHERE normalized_name=:normalized_name AND is_provisional=1
ORDER BY id LIMIT 1;

