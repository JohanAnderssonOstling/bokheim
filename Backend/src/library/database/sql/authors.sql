-- Stable author identities and ordered book credits/contributors.

-- name: existing_book_contributor_credit?
-- param: book_id: &str
-- param: position: i64
SELECT author_identity.id, author_identity.stable_id, book_contributor.name
FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
WHERE book_contributor.book_id=:book_id AND book_contributor.position=:position;

-- name: author_identity_by_stable_id?
-- param: stable_id: &str
SELECT id, stable_id FROM author_identity WHERE stable_id=:stable_id;

-- name: provisional_author_identity_by_name?
-- param: normalized_name: &str
SELECT id, stable_id
FROM author_identity
WHERE normalized_name=:normalized_name AND is_provisional=1
ORDER BY id LIMIT 1;

-- name: insert_author_identity!
-- param: stable_id: &str
-- param: preferred_name: &str
-- param: normalized_name: &str
-- param: created_at: i64
INSERT INTO author_identity(stable_id, preferred_name, normalized_name, is_provisional, created_at)
VALUES(:stable_id, :preferred_name, :normalized_name, 1, :created_at);

-- name: upsert_book_contributor!
-- param: book_id: &str
-- param: author_identity_id: i64
-- param: position: i64
-- param: name: &str
-- param: sort_value: Option<&str>
-- param: role: &[u8]
INSERT INTO book_contributor(book_id, author_identity_id, position, name, sort_name, role)
VALUES(:book_id, :author_identity_id, :position, :name, :sort_value, :role)
ON CONFLICT(book_id, position) DO UPDATE SET
    author_identity_id=excluded.author_identity_id,
    name=excluded.name,
    sort_name=excluded.sort_name,
    role=excluded.role;

-- name: trim_book_contributors!
-- param: book_id: &str
-- param: first_removed_position: i64
DELETE FROM book_contributor WHERE book_id=:book_id AND position>=:first_removed_position;
