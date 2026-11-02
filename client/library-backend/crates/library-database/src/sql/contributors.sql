-- Resolve contributor identities and persisted name keys.

-- name: contributors_insert_name_key !
-- param: name_key: &str
-- param: author_identity_id: i64
INSERT OR IGNORE INTO author_name_key(name_key,author_identity_id) VALUES(:name_key,:author_identity_id);

-- name: contributors_identity_by_name_key ?
-- param: name_key: &str
SELECT identity.id,identity.stable_id
FROM author_name_key keys JOIN author_identity identity ON identity.id=keys.author_identity_id
WHERE keys.name_key=:name_key AND identity.is_provisional=1
ORDER BY keys.author_identity_id LIMIT 1;

-- name: contributors_existing_names ?
SELECT id,name FROM author_identity;
