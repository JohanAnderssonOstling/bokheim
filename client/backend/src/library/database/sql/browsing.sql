-- Library-facing read models for paths, folders, book grids, and authors.

-- name: live_directory_exists?
-- param: directory_id: &str
SELECT EXISTS(
    SELECT 1 FROM dir WHERE id=:directory_id AND deleted_at IS NULL
);

-- name: get_book_paths?
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.deleted_at IS NULL;

-- name: get_book_assets?
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT
    book.content_hash,
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path,
    bd.is_downloaded
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
JOIN book ON book.row_id = bd.book_row_id
WHERE bd.deleted_at IS NULL AND book.content_hash != '';

-- name: get_book_paths_without_title?
SELECT
    b.content_hash,
    CASE
        WHEN dp.path = ''
            THEN bd.file_name
        ELSE dp.path || '/' || bd.file_name
    END AS full_path
FROM book b
JOIN book_dir bd ON bd.book_row_id = b.row_id
JOIN dir_paths dp ON dp.id = bd.dir_id
WHERE b.deleted_at IS NULL
  AND (b.title IS NULL OR NOT EXISTS (SELECT 1 FROM author_credit WHERE book_row_id = b.row_id))
  AND bd.deleted_at IS NULL;




-- name: get_dir_paths?
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT path FROM dir_paths ORDER BY path;

-- name: get_sub_dirs?
-- param: dir_id: &str
WITH RECURSIVE descendants(root_id, id) AS (
    SELECT directory.id, directory.id
    FROM dir AS directory
    WHERE directory.parent_id = :dir_id
      AND directory.id != :dir_id
      AND directory.deleted_at IS NULL
    UNION
    SELECT descendants.root_id, child.id
    FROM descendants
    JOIN dir AS child ON child.parent_id = descendants.id
    WHERE child.id != descendants.id
      AND child.deleted_at IS NULL
)
SELECT
    root.id,
    root.name,
    COUNT(DISTINCT book.row_id) AS book_count,
    COUNT(DISTINCT CASE WHEN placement.is_downloaded = 1 THEN book.row_id END) AS downloaded_book_count
FROM dir AS root
JOIN descendants ON descendants.root_id = root.id
LEFT JOIN book_dir AS placement ON placement.dir_id = descendants.id AND placement.deleted_at IS NULL
LEFT JOIN book ON book.row_id = placement.book_row_id AND book.deleted_at IS NULL AND book.hidden_at IS NULL
WHERE root.parent_id = :dir_id
  AND root.id != :dir_id
  AND root.deleted_at IS NULL
GROUP BY root.id, root.name;

-- Card-shaped reads share the book_cards view; each query only owns scope and ordering.

-- name: get_subject_children?
-- param: parent_path: &str
WITH RECURSIVE
eligible_books(book_row_id, downloaded) AS (
    SELECT book.row_id, MAX(placement.is_downloaded)
    FROM book
    JOIN book_dir placement
      ON placement.book_row_id = book.row_id
     AND placement.deleted_at IS NULL
    WHERE book.deleted_at IS NULL
      AND book.hidden_at IS NULL
    GROUP BY book.row_id
), descendants(root_path,path,concept_id) AS (
    SELECT route.path,route.path,route.concept_id
    FROM unified_concept_route route
    WHERE route.parent_path=:parent_path
    UNION ALL
    SELECT descendants.root_path,child.path,child.concept_id
    FROM descendants
    JOIN unified_concept_route child ON child.parent_path=descendants.path
)
SELECT
    root.path,
    root.label,
    COUNT(DISTINCT eligible.book_row_id) AS book_count,
    COUNT(DISTINCT CASE WHEN eligible.downloaded = 1 THEN eligible.book_row_id END) AS downloaded_book_count
FROM unified_concept_route root
LEFT JOIN descendants ON descendants.root_path=root.path
LEFT JOIN book_unified_concept assignment ON assignment.concept_id=descendants.concept_id
LEFT JOIN eligible_books eligible ON eligible.book_row_id=assignment.book_row_id
WHERE root.parent_path=:parent_path
GROUP BY root.path,root.label
ORDER BY LOWER(root.label),root.path;

-- name: get_uncategorized_subject_counts?
SELECT
    COUNT(DISTINCT book.row_id),
    COUNT(DISTINCT CASE WHEN EXISTS(
        SELECT 1 FROM book_dir downloaded
        WHERE downloaded.book_row_id=book.row_id AND downloaded.deleted_at IS NULL AND downloaded.is_downloaded=1
    ) THEN book.row_id END)
FROM book
WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
  AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL)
  AND NOT EXISTS(SELECT 1 FROM book_subject subject WHERE subject.book_row_id=book.row_id)
  AND NOT EXISTS(SELECT 1 FROM book_external_entity entity WHERE entity.book_row_id=book.row_id);

-- name: get_other_subject_system_counts?
SELECT
    COUNT(DISTINCT book.row_id),
    COUNT(DISTINCT CASE WHEN EXISTS(
        SELECT 1 FROM book_dir downloaded
        WHERE downloaded.book_row_id=book.row_id AND downloaded.deleted_at IS NULL AND downloaded.is_downloaded=1
    ) THEN book.row_id END)
FROM book
WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
  AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL)
  AND NOT EXISTS(
      SELECT 1 FROM book_subject_assignment unified
      WHERE unified.book_row_id=book.row_id AND unified.system_id='unified'
  )
  AND EXISTS(
      SELECT 1 FROM book_subject subject
      WHERE subject.book_row_id=book.row_id
        AND LOWER(COALESCE(TRIM(subject.authority), '')) NOT IN ('wikidata-place', 'wikidata-person')
  );

-- name: get_book_cards_in_subject?
-- param: subject_path: &str
-- param: downloaded_only: i32
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0)
FROM book_cards card
JOIN book_unified_concept assignment ON assignment.book_row_id=card.book_row_id
JOIN unified_concept_route route ON route.concept_id=assignment.concept_id
WHERE route.path=:subject_path
  AND (:downloaded_only=0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_book_cards_in_subject_tree?
-- param: subject_path: &str
-- param: downloaded_only: i32
WITH RECURSIVE routes(path,concept_id) AS (
    SELECT path,concept_id FROM unified_concept_route WHERE path=:subject_path
    UNION ALL
    SELECT child.path,child.concept_id
    FROM routes parent
    JOIN unified_concept_route child ON child.parent_path=parent.path
)
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0)
FROM book_cards card
JOIN book_unified_concept assignment ON assignment.book_row_id=card.book_row_id
JOIN routes ON routes.concept_id=assignment.concept_id
WHERE (:downloaded_only=0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_uncategorized_book_cards?
-- param: downloaded_only: i32
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0)
FROM book_cards card
WHERE NOT EXISTS(
    SELECT 1 FROM book_subject subject
    WHERE subject.book_row_id=card.book_row_id
)
  AND NOT EXISTS(SELECT 1 FROM book_external_entity entity WHERE entity.book_row_id=card.book_row_id)
  AND (:downloaded_only=0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_other_subject_system_book_cards?
-- param: downloaded_only: i32
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0)
FROM book_cards card
WHERE NOT EXISTS(
    SELECT 1 FROM book_subject_assignment unified
    WHERE unified.book_row_id=card.book_row_id AND unified.system_id='unified'
)
  AND EXISTS(
    SELECT 1 FROM book_subject subject
    WHERE subject.book_row_id=card.book_row_id
      AND LOWER(COALESCE(TRIM(subject.authority), '')) NOT IN ('wikidata-place', 'wikidata-person')
)
  AND (:downloaded_only=0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: search_book_cards_by_title?
-- param: normalized_query: &str
-- param: downloaded_only: i32
SELECT content_hash, title, author, description, progress, downloaded, download_requested,
       audiobook, audiobook_duration_ms, audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=book_cards.book_row_id), 0)
FROM book_cards
WHERE instr(normalize_search_text(COALESCE(title, '')), :normalized_query) > 0
  AND (:downloaded_only = 0 OR downloaded)
ORDER BY normalize_search_text(COALESCE(title, '')), content_hash;

-- name: search_book_cards_by_author?
-- param: normalized_query: &str
-- param: downloaded_only: i32
SELECT content_hash, title, author, description, progress, downloaded, download_requested,
       audiobook, audiobook_duration_ms, audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=book_cards.book_row_id), 0)
FROM book_cards
WHERE instr(normalize_search_text(COALESCE(author, '')), :normalized_query) > 0
  AND instr(normalize_search_text(COALESCE(title, '')), :normalized_query) = 0
  AND (:downloaded_only = 0 OR downloaded)
ORDER BY normalize_search_text(COALESCE(title, '')), content_hash;

-- name: get_book_cards_in_dir?
-- param: dir_id: &str
-- param: downloaded_only: i32
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0)
FROM book_cards card
JOIN book_dir placement ON placement.book_row_id = card.book_row_id
WHERE placement.dir_id = :dir_id AND placement.deleted_at IS NULL
  AND (:downloaded_only = 0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_book_cards_by_author?
-- param: stable_id: &str
-- param: downloaded_only: i32
-- DISTINCT because `author_credit` is keyed by position: one book crediting the
-- same identity twice — two spellings that normalized together — would
-- otherwise appear twice in the grid.
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0)
FROM book_cards card
JOIN author_credit credit ON credit.book_row_id = card.book_row_id
JOIN author_identity identity ON identity.id = credit.author_identity_id
WHERE identity.stable_id = :stable_id
  AND (:downloaded_only = 0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_descendant_content_hashes?
-- param: dir_id: &str
WITH RECURSIVE subtree(id) AS (
    SELECT id FROM dir WHERE id = :dir_id AND deleted_at IS NULL
    UNION
    SELECT child.id
    FROM dir child
    JOIN subtree parent ON child.parent_id = parent.id
    WHERE child.id != child.parent_id
      AND child.deleted_at IS NULL
)
SELECT DISTINCT book.content_hash
FROM book_dir placement
JOIN subtree ON subtree.id = placement.dir_id
JOIN book ON book.row_id = placement.book_row_id
WHERE placement.deleted_at IS NULL
  AND book.deleted_at IS NULL
  AND book.hidden_at IS NULL
ORDER BY book.content_hash;

-- name: get_descendant_subject_content_hashes?
-- param: subject_path: &str
WITH RECURSIVE routes(path,concept_id) AS (
    SELECT path,concept_id FROM unified_concept_route WHERE path=:subject_path
    UNION ALL
    SELECT child.path,child.concept_id
    FROM routes parent
    JOIN unified_concept_route child ON child.parent_path=parent.path
)
SELECT DISTINCT book.content_hash
FROM routes
JOIN book_unified_concept assignment ON assignment.concept_id=routes.concept_id
JOIN book ON book.row_id = assignment.book_row_id
WHERE book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND EXISTS(
      SELECT 1 FROM book_dir placement
      WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
  )
ORDER BY book.content_hash;

-- name: get_external_place_roots?
WITH RECURSIVE roots(wikidata_id) AS (
    SELECT entity.wikidata_id FROM external_entity entity
    WHERE entity.kind='place' AND NOT EXISTS(SELECT 1 FROM external_place_edge edge WHERE edge.child_id=entity.wikidata_id)
), reachable(wikidata_id) AS (
    SELECT wikidata_id FROM roots
    UNION
    SELECT edge.child_id FROM external_place_edge edge JOIN reachable parent ON edge.parent_id=parent.wikidata_id
), visible_roots(wikidata_id) AS (
    SELECT wikidata_id FROM roots
    UNION
    -- A cyclic component has no structural root. Surface its directly used
    -- nodes at the top so malformed Wikidata edges cannot hide library books.
    SELECT DISTINCT reference.wikidata_id FROM book_external_entity reference
    WHERE reference.kind='place' AND reference.wikidata_id NOT IN (SELECT wikidata_id FROM reachable)
)
SELECT entity.wikidata_id, entity.label
FROM visible_roots
JOIN external_entity entity ON entity.wikidata_id=visible_roots.wikidata_id
WHERE EXISTS(
    WITH RECURSIVE descendants(wikidata_id) AS (
        SELECT entity.wikidata_id
        UNION
        SELECT edge.child_id FROM external_place_edge edge JOIN descendants parent ON edge.parent_id=parent.wikidata_id
    )
    SELECT 1 FROM descendants JOIN book_external_entity reference ON reference.wikidata_id=descendants.wikidata_id WHERE reference.kind='place'
)
ORDER BY LOWER(entity.label), entity.wikidata_id;

-- name: get_external_place_children?
-- param: parent_id: &str
SELECT DISTINCT entity.wikidata_id, entity.label
FROM external_place_edge edge
JOIN external_entity entity ON entity.wikidata_id=edge.child_id AND entity.kind='place'
WHERE edge.parent_id=:parent_id
ORDER BY LOWER(entity.label), entity.wikidata_id;

-- name: get_external_people?
SELECT entity.wikidata_id, entity.label
FROM external_entity entity
WHERE entity.kind='person'
  AND EXISTS(SELECT 1 FROM book_external_entity reference WHERE reference.wikidata_id=entity.wikidata_id AND reference.kind='person')
ORDER BY LOWER(entity.label), entity.wikidata_id;

-- name: get_external_places?
SELECT entity.wikidata_id, entity.label
FROM external_entity entity
WHERE entity.kind='place'
  AND EXISTS(SELECT 1 FROM book_external_entity reference WHERE reference.wikidata_id=entity.wikidata_id AND reference.kind='place')
ORDER BY LOWER(entity.label), entity.wikidata_id;

-- name: get_external_entity_book_counts?
-- param: wikidata_id: &str
-- param: recursive_places: i32
WITH RECURSIVE descendants(wikidata_id) AS (
    SELECT :wikidata_id
    UNION
    SELECT edge.child_id
    FROM external_place_edge edge
    JOIN descendants parent ON :recursive_places=1 AND edge.parent_id=parent.wikidata_id
)
SELECT COUNT(DISTINCT reference.book_row_id),
       COUNT(DISTINCT CASE WHEN card.downloaded THEN reference.book_row_id END)
FROM descendants
JOIN book_external_entity reference ON reference.wikidata_id=descendants.wikidata_id
JOIN book_cards card ON card.book_row_id=reference.book_row_id;

-- name: get_book_cards_for_external_entity?
-- param: wikidata_id: &str
-- param: downloaded_only: i32
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0)
FROM book_cards card
JOIN book_external_entity reference ON reference.book_row_id=card.book_row_id
WHERE reference.wikidata_id=:wikidata_id
  AND (:downloaded_only=0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_descendant_external_place_content_hashes?
-- param: wikidata_id: &str
WITH RECURSIVE descendants(wikidata_id) AS (
    SELECT :wikidata_id
    UNION
    SELECT edge.child_id
    FROM external_place_edge edge
    JOIN descendants parent ON edge.parent_id=parent.wikidata_id
)
SELECT DISTINCT book.content_hash
FROM descendants
JOIN book_external_entity reference ON reference.wikidata_id=descendants.wikidata_id AND reference.kind='place'
JOIN book ON book.row_id=reference.book_row_id
WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
ORDER BY book.content_hash;

-- name: get_folder_download_counts?
-- param: dir_id: &str
WITH RECURSIVE subtree(id) AS (
    SELECT id FROM dir WHERE id = :dir_id AND deleted_at IS NULL
    UNION
    SELECT child.id
    FROM dir child
    JOIN subtree parent ON child.parent_id = parent.id
    WHERE child.id != child.parent_id
      AND child.deleted_at IS NULL
)
SELECT
    COUNT(DISTINCT placement.book_row_id),
    COUNT(DISTINCT CASE WHEN placement.is_downloaded = 1 THEN placement.book_row_id END)
FROM book_dir placement
JOIN subtree ON subtree.id = placement.dir_id
JOIN book ON book.row_id = placement.book_row_id
WHERE placement.deleted_at IS NULL
  AND book.deleted_at IS NULL
  AND book.hidden_at IS NULL;

-- name: get_book_ancestor_dir_ids?
-- param: content_hash: &str
WITH RECURSIVE ancestors(id) AS (
    SELECT placement.dir_id
    FROM book_dir placement
    WHERE placement.book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
      AND placement.deleted_at IS NULL
    UNION
    SELECT directory.parent_id
    FROM dir directory
    JOIN ancestors ON ancestors.id = directory.id
    WHERE directory.id != '00000000-0000-0000-0000-000000000000'
      AND directory.deleted_at IS NULL
)
SELECT id FROM ancestors ORDER BY id;

-- name: get_authors?
-- param: downloaded_only: i32
-- `sort_name` is the filing form — "Ekman, Kerstin" — taken from the EPUB's
-- `file-as` where the book carries one. It is NULL when no credit for this
-- identity has one, and the caller falls back to the display name; without it
-- the index files one person under two letters, once per spelling.
SELECT
    author_identity.stable_id AS author_id,
    MIN(author_credit.credited_name) AS display_name,
    MIN(NULLIF(TRIM(COALESCE(author_credit.sort_name, '')), '')) AS sort_name,
    COUNT(DISTINCT book.row_id) AS book_count
FROM author_identity
JOIN author_credit ON author_credit.author_identity_id = author_identity.id
JOIN book ON book.row_id = author_credit.book_row_id
JOIN book_dir ON book_dir.book_row_id = book.row_id
WHERE book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND book_dir.deleted_at IS NULL
  AND TRIM(author_credit.credited_name) != ''
  AND (:downloaded_only = 0 OR EXISTS (
      SELECT 1 FROM book_dir available
      WHERE available.book_row_id = book.row_id
        AND available.deleted_at IS NULL
        AND available.is_downloaded = 1
  ))
GROUP BY author_identity.id, author_identity.stable_id
ORDER BY LOWER(display_name);

-- name: get_author_cover_hashes?
-- param: downloaded_only: i32
-- param: limit: i32
-- The covers an author's row shows: their most recently read books first, then
-- by title, capped per author. One statement for the whole index rather than a
-- correlated lookup per row, so the cost does not scale with the list.
WITH credited AS (
    SELECT author_identity.stable_id AS author_id,
           card.content_hash AS content_hash,
           MIN(card.title) AS title,
           (SELECT reading.changed_at FROM sync_state_version reading
             WHERE reading.state_kind = 'reading_position'
               AND reading.state_key = card.content_hash) AS read_at
    FROM author_identity
    JOIN author_credit ON author_credit.author_identity_id = author_identity.id
    JOIN book_cards card ON card.book_row_id = author_credit.book_row_id
    WHERE (:downloaded_only = 0 OR card.downloaded)
    -- One book credited twice to the same identity is still one cover.
    GROUP BY author_identity.stable_id, card.content_hash
)
SELECT author_id, content_hash, title
FROM (
    SELECT author_id, content_hash, title,
           ROW_NUMBER() OVER (
               PARTITION BY author_id
               ORDER BY COALESCE(read_at, 0) DESC, LOWER(COALESCE(title, '')), content_hash
           ) AS position
    FROM credited
)
WHERE position <= :limit
ORDER BY author_id, position;

-- name: get_author_discovery_seed?
-- param: stable_id: &str
SELECT
    MIN(author_credit.credited_name) AS display_name,
    book_identifier.scheme,
    COALESCE(book_identifier.canonical_value, book_identifier.value) AS identifier_value
FROM author_identity
JOIN author_credit ON author_credit.author_identity_id = author_identity.id
JOIN book ON book.row_id = author_credit.book_row_id
JOIN book_dir ON book_dir.book_row_id = book.row_id
LEFT JOIN book_identifier ON book_identifier.book_row_id = book.row_id
WHERE author_identity.stable_id = :stable_id
  AND book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND book_dir.deleted_at IS NULL
  AND TRIM(author_credit.credited_name) != ''
GROUP BY book_identifier.scheme, COALESCE(book_identifier.canonical_value, book_identifier.value)
ORDER BY book_identifier.scheme, identifier_value;
