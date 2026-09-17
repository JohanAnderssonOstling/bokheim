-- Browse read models and subject projection maintenance.

-- Core book, directory, and local placement write operations.

-- name: upsert_dir!
-- param: id: &str
-- param: parent_id: &str
-- param: name: &str
INSERT INTO dir (id, parent_id, name, intent_parent_id, intent_name, intent_lifecycle, deleted_at, purged_at)
VALUES (:id, :parent_id, :name, :parent_id, :name, 0, NULL, NULL)
ON CONFLICT(id) DO UPDATE SET
    parent_id = excluded.parent_id,
    name = excluded.name,
    intent_parent_id = excluded.parent_id,
    intent_name = excluded.name,
    intent_lifecycle = 0,
    deleted_at = NULL,
    purged_at = NULL;

-- name: insert_book_dir!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: local_hash: &str
-- param: last_scan: i64
-- param: is_downloaded: i32
INSERT INTO book_dir    (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at)
VALUES                  (:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,:local_hash,:last_scan,:is_downloaded,NULL)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    deleted_at = NULL,
    trash_origin_dir_id = NULL,
    -- max() so a remote apply (last_scan 0) can't lower a live scan stamp and
    -- make the next sweep delete a present file.
    last_scan = max(book_dir.last_scan, excluded.last_scan),
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded),
    local_hash = excluded.local_hash;

-- Library-facing read models for paths, folders, book grids, and authors.

-- name: get_immediate_sub_dirs?
-- param: dir_id: &str
-- param: chip_sort: i32
WITH RECURSIVE visible_tree(folder_id,id) AS (
    SELECT directory.id,directory.id
    FROM dir AS parent
    JOIN dir AS directory ON directory.parent_id=parent.id AND directory.id!=parent.id
    WHERE parent.id=:dir_id AND parent.deleted_at IS NULL
      AND directory.deleted_at IS NULL AND directory.name NOT LIKE '.%'
    UNION ALL
    SELECT parent.folder_id,child.id
    FROM visible_tree AS parent
    JOIN dir AS child ON child.parent_id=parent.id AND child.id!=parent.id
    WHERE child.deleted_at IS NULL AND child.name NOT LIKE '.%'
)
SELECT directory.id,directory.name,
    COUNT(DISTINCT book.row_id) AS book_count,
    COUNT(DISTINCT CASE WHEN placement.is_downloaded THEN book.row_id END) AS downloaded_book_count,
    NULL AS path
FROM visible_tree
JOIN dir AS directory ON directory.id=visible_tree.folder_id
-- Left joins so a folder holding nothing still lists, with a count of zero: a
-- folder the reader has just created is empty by definition, and a listing that
-- hides it looks like the creation failed. Dot-directories stay hidden, and the
-- conditions stay in the ON clauses so they filter the counts rather than
-- turning these back into inner joins.
LEFT JOIN book_dir AS placement ON placement.dir_id=visible_tree.id AND placement.deleted_at IS NULL
LEFT JOIN book ON book.row_id=placement.book_row_id AND book.deleted_at IS NULL AND book.hidden_at IS NULL
GROUP BY directory.id,directory.name
ORDER BY
    CASE WHEN :chip_sort=1 THEN book_count END DESC,
    normalize_search_text(directory.name),directory.name,directory.id;

-- Search and metadata filters can match descendants and need breadcrumb paths,
-- so they retain the recursive form.
-- name: get_sub_dirs?
-- param: dir_id: &str
-- param: normalized_query: &str
-- param: file_types: i32
-- param: chip_sort: i32
-- param: languages: &str
WITH RECURSIVE
scope(id) AS (
    SELECT id
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION
    SELECT child.id
    FROM dir AS child
    JOIN scope AS parent ON child.parent_id = parent.id
    WHERE child.id != parent.id AND child.deleted_at IS NULL AND child.name NOT LIKE '.%'
),
candidate_dirs(id,name,path) AS (
    SELECT
        directory.id,
        directory.name,
        CASE WHEN :normalized_query = '' THEN NULL ELSE parent_path.path END
    FROM dir AS directory
    JOIN scope ON scope.id=directory.id
    LEFT JOIN dir_paths AS parent_path ON parent_path.id=directory.parent_id
    WHERE directory.id != :dir_id
      AND (
          (:normalized_query = '' AND directory.parent_id = :dir_id)
          OR (:normalized_query != '' AND instr(normalize_search_text(COALESCE(directory.name, '')), :normalized_query) > 0)
      )
),
visible_tree(folder_id,id) AS (
    SELECT id,id FROM candidate_dirs
    UNION ALL
    SELECT parent.folder_id,child.id
    FROM visible_tree AS parent
    JOIN dir AS child ON child.parent_id=parent.id AND child.id!=parent.id
    WHERE child.deleted_at IS NULL AND child.name NOT LIKE '.%'
),
folder_rows AS (
    SELECT
        candidate.id,
        candidate.name,
        candidate.path,
        COUNT(DISTINCT book.row_id) AS book_count,
        COUNT(DISTINCT CASE WHEN placement.is_downloaded THEN book.row_id END) AS downloaded_book_count
    FROM candidate_dirs AS candidate
    JOIN visible_tree ON visible_tree.folder_id=candidate.id
    JOIN book_dir AS placement ON placement.dir_id=visible_tree.id AND placement.deleted_at IS NULL
    JOIN book ON book.row_id=placement.book_row_id
        AND book.deleted_at IS NULL
        AND book.hidden_at IS NULL
    WHERE (
            :file_types = 0
            OR ((:file_types & 1) != 0 AND LOWER(book.format) NOT IN ('pdf','m4b'))
            OR ((:file_types & 2) != 0 AND LOWER(book.format) = 'pdf')
            OR ((:file_types & 4) != 0 AND LOWER(book.format) = 'm4b')
        )
        AND (
            :languages = ''
            OR EXISTS(
                SELECT 1 FROM book_language
                WHERE book_language.book_row_id=book.row_id
                  AND (
                      instr(:languages, ',' || LOWER(book_language.language_tag) || ',') > 0
                      OR (
                          instr(LOWER(book_language.language_tag), '-') > 0
                          AND instr(
                              :languages,
                              ',' || substr(LOWER(book_language.language_tag), 1, instr(LOWER(book_language.language_tag), '-') - 1) || ','
                          ) > 0
                      )
                  )
            )
        )
    GROUP BY candidate.id,candidate.name,candidate.path
    HAVING COUNT(*) > 0
)
SELECT id,name,book_count,downloaded_book_count,path
FROM folder_rows
ORDER BY
    CASE WHEN :chip_sort=1 THEN book_count END DESC,
    normalize_search_text(name),name,id;

-- BrowsePage renders database order directly. Hierarchy rows and cards default
-- to alphabetical order; their independent sort parameters can promote book
-- count and reading recency respectively.

-- name: get_folder_language_facets?
-- param: dir_id: &str
-- param: normalized_query: &str
-- param: file_types: i32
-- param: downloaded_only: i32
SELECT language.base_language,COUNT(DISTINCT ownership.book_row_id)
FROM folder_book_ownership AS ownership
JOIN book_cards AS card ON card.book_row_id=ownership.book_row_id
JOIN book_browse_format AS format ON format.book_row_id=ownership.book_row_id
JOIN book_browse_language AS language ON language.book_row_id=ownership.book_row_id
WHERE ownership.folder_id=:dir_id
  AND (:downloaded_only=0 OR card.downloaded)
  AND (:file_types=0 OR (:file_types & format.format_category) != 0)
  AND (
      :normalized_query=''
      OR instr(normalize_search_text(COALESCE(card.title,'') || COALESCE(': ' || card.subtitle,'')),:normalized_query)>0
      OR instr(normalize_search_text(COALESCE(card.author,'')),:normalized_query)>0
      OR EXISTS(
          SELECT 1
          FROM dir AS candidate
          JOIN dir_paths AS candidate_path ON candidate_path.id=candidate.id
          JOIN dir_paths AS current_path ON current_path.id=:dir_id
          JOIN folder_book_ownership AS candidate_ownership
            ON candidate_ownership.folder_id=candidate.id
           AND candidate_ownership.book_row_id=ownership.book_row_id
          WHERE candidate.id!=:dir_id
            AND (current_path.path='' OR substr(candidate_path.path,1,length(current_path.path)+1)=current_path.path || '/')
            AND instr(normalize_search_text(COALESCE(candidate.name,'')),:normalized_query)>0
      )
  )
GROUP BY language.base_language ORDER BY language.base_language;

-- Format facets apply the current folder, search and language constraints,
-- while deliberately ignoring the format selection itself.
-- name: get_folder_format_facets?
-- param: dir_id: &str
-- param: normalized_query: &str
-- param: languages: &str
-- param: downloaded_only: i32
SELECT format.format_category,COUNT(DISTINCT ownership.book_row_id)
FROM folder_book_ownership AS ownership
JOIN book_cards AS card ON card.book_row_id=ownership.book_row_id
JOIN book_browse_format AS format ON format.book_row_id=ownership.book_row_id
WHERE ownership.folder_id=:dir_id
  AND (:downloaded_only=0 OR card.downloaded)
  AND (:languages='' OR EXISTS(SELECT 1 FROM book_browse_language AS language WHERE language.book_row_id=ownership.book_row_id AND instr(:languages,',' || language.base_language || ',')>0))
  AND (
      :normalized_query=''
      OR instr(normalize_search_text(COALESCE(card.title,'') || COALESCE(': ' || card.subtitle,'')),:normalized_query)>0
      OR instr(normalize_search_text(COALESCE(card.author,'')),:normalized_query)>0
      OR EXISTS(
          SELECT 1
          FROM dir AS candidate
          JOIN dir_paths AS candidate_path ON candidate_path.id=candidate.id
          JOIN dir_paths AS current_path ON current_path.id=:dir_id
          JOIN folder_book_ownership AS candidate_ownership
            ON candidate_ownership.folder_id=candidate.id
           AND candidate_ownership.book_row_id=ownership.book_row_id
          WHERE candidate.id!=:dir_id
            AND (current_path.path='' OR substr(candidate_path.path,1,length(current_path.path)+1)=current_path.path || '/')
            AND instr(normalize_search_text(COALESCE(candidate.name,'')),:normalized_query)>0
      )
  )
GROUP BY format.format_category ORDER BY format.format_category;

-- name: get_subject_children?
-- param: parent_path: &str
-- param: normalized_query: &str
-- param: file_types: i32
-- param: chip_sort: i32
-- param: languages: &str
WITH RECURSIVE
scope(path,parent_path,concept_id) AS (
    SELECT route.path,route.parent_path,route.concept_id
    FROM curated.unified_concept_route AS route
    WHERE route.path=:parent_path OR route.parent_path=:parent_path
    UNION
    SELECT child.path,child.parent_path,child.concept_id
    FROM curated.unified_concept_route AS child
    JOIN scope AS parent ON child.parent_path=parent.path
),
candidate_subjects(path,label,parent_path) AS (
    SELECT
        route.path,
        route.label,
        CASE WHEN :normalized_query = '' THEN NULL ELSE route.parent_path END
    FROM curated.unified_concept_route AS route
    JOIN scope ON scope.path=route.path
    WHERE route.path != :parent_path
      AND (
          (:normalized_query = '' AND route.parent_path=:parent_path)
          OR (:normalized_query != '' AND instr(normalize_search_text(COALESCE(route.label, '')), :normalized_query) > 0)
      )
),
candidate_descendants(root_path,path,concept_id) AS (
    SELECT candidate.path,route.path,route.concept_id
    FROM candidate_subjects AS candidate
    JOIN curated.unified_concept_route AS route ON route.path=candidate.path
    UNION ALL
    SELECT descendants.root_path,child.path,child.concept_id
    FROM candidate_descendants AS descendants
    JOIN curated.unified_concept_route AS child ON child.parent_path=descendants.path
),
subject_rows AS (
    SELECT
        candidate.path,
        candidate.label,
        candidate.parent_path,
        COUNT(DISTINCT book.row_id) AS book_count,
        COUNT(DISTINCT CASE WHEN card.downloaded THEN book.row_id END) AS downloaded_book_count
    FROM candidate_subjects AS candidate
    JOIN candidate_descendants AS descendants ON descendants.root_path=candidate.path
    LEFT JOIN book_unified_concept AS assignment ON assignment.concept_id=descendants.concept_id
    LEFT JOIN book ON book.row_id=assignment.book_row_id
        AND book.deleted_at IS NULL
        AND book.hidden_at IS NULL
        AND (
            :file_types = 0
            OR ((:file_types & 1) != 0 AND LOWER(book.format) NOT IN ('pdf','m4b'))
            OR ((:file_types & 2) != 0 AND LOWER(book.format) = 'pdf')
            OR ((:file_types & 4) != 0 AND LOWER(book.format) = 'm4b')
        )
        AND (
            :languages = ''
            OR EXISTS(
                SELECT 1 FROM book_language
                WHERE book_language.book_row_id=book.row_id
                  AND (
                      instr(:languages, ',' || LOWER(book_language.language_tag) || ',') > 0
                      OR (
                          instr(LOWER(book_language.language_tag), '-') > 0
                          AND instr(
                              :languages,
                              ',' || substr(LOWER(book_language.language_tag), 1, instr(LOWER(book_language.language_tag), '-') - 1) || ','
                          ) > 0
                      )
                  )
            )
        )
        AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL)
    LEFT JOIN book_cards AS card ON card.book_row_id=book.row_id
    GROUP BY candidate.path,candidate.label,candidate.parent_path
    HAVING COUNT(DISTINCT book.row_id) > 0
)
SELECT path,label,book_count,downloaded_book_count,parent_path
FROM subject_rows
ORDER BY
    CASE WHEN :chip_sort=1 THEN book_count END DESC,
    normalize_search_text(label),label,path;

-- Subject facets use the same derived dimensions and cross-filter rules as
-- folder facets; ownership already includes descendant assignments.
-- name: get_subject_language_facets?
-- param: subject_path: &str
-- param: normalized_query: &str
-- param: file_types: i32
-- param: downloaded_only: i32
SELECT language.base_language,COUNT(DISTINCT ownership.book_row_id)
FROM subject_book_ownership AS ownership
JOIN book_cards AS card ON card.book_row_id=ownership.book_row_id
JOIN book_browse_format AS format ON format.book_row_id=ownership.book_row_id
JOIN book_browse_language AS language ON language.book_row_id=ownership.book_row_id
WHERE ownership.subject_path=:subject_path
  AND (:downloaded_only=0 OR card.downloaded)
  AND (:file_types=0 OR (:file_types & format.format_category) != 0)
  AND (
      :normalized_query=''
      OR instr(normalize_search_text(COALESCE(card.title,'') || COALESCE(': ' || card.subtitle,'')),:normalized_query)>0
      OR instr(normalize_search_text(COALESCE(card.author,'')),:normalized_query)>0
      OR EXISTS(
          SELECT 1
          FROM subject_book_ownership AS candidate_ownership
          JOIN library_subject AS candidate ON candidate.path=candidate_ownership.subject_path
          WHERE candidate_ownership.book_row_id=ownership.book_row_id
            AND candidate.path!=:subject_path
            AND (:subject_path='Subject' OR substr(candidate.path,1,length(:subject_path)+3)=:subject_path || ' / ')
            AND instr(normalize_search_text(COALESCE(candidate.label,'')),:normalized_query)>0
      )
  )
GROUP BY language.base_language ORDER BY language.base_language;

-- name: get_subject_format_facets?
-- param: subject_path: &str
-- param: normalized_query: &str
-- param: languages: &str
-- param: downloaded_only: i32
SELECT format.format_category,COUNT(DISTINCT ownership.book_row_id)
FROM subject_book_ownership AS ownership
JOIN book_cards AS card ON card.book_row_id=ownership.book_row_id
JOIN book_browse_format AS format ON format.book_row_id=ownership.book_row_id
WHERE ownership.subject_path=:subject_path
  AND (:downloaded_only=0 OR card.downloaded)
  AND (:languages='' OR EXISTS(SELECT 1 FROM book_browse_language AS language WHERE language.book_row_id=ownership.book_row_id AND instr(:languages,',' || language.base_language || ',')>0))
  AND (
      :normalized_query=''
      OR instr(normalize_search_text(COALESCE(card.title,'') || COALESCE(': ' || card.subtitle,'')),:normalized_query)>0
      OR instr(normalize_search_text(COALESCE(card.author,'')),:normalized_query)>0
      OR EXISTS(
          SELECT 1
          FROM subject_book_ownership AS candidate_ownership
          JOIN library_subject AS candidate ON candidate.path=candidate_ownership.subject_path
          WHERE candidate_ownership.book_row_id=ownership.book_row_id
            AND candidate.path!=:subject_path
            AND (:subject_path='Subject' OR substr(candidate.path,1,length(:subject_path)+3)=:subject_path || ' / ')
            AND instr(normalize_search_text(COALESCE(candidate.label,'')),:normalized_query)>0
      )
  )
GROUP BY format.format_category ORDER BY format.format_category;

-- name: get_book_cards_in_subject?
-- param: subject_path: &str
-- param: downloaded_only: i32
-- param: normalized_query: &str
-- param: file_types: i32
-- param: book_sort: i32
-- param: languages: &str
-- param: include_descendants: i32
WITH RECURSIVE scope(path,concept_id) AS (
    SELECT route.path,route.concept_id
    FROM library_subject AS route
    WHERE route.path=:subject_path OR route.parent_path=:subject_path
    UNION
    SELECT child.path,child.concept_id
    FROM library_subject AS child
    JOIN scope AS parent ON child.parent_path=parent.path
)
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE(book.added_at, 0), card.subtitle
FROM book_cards card
JOIN book ON book.row_id=card.book_row_id
JOIN book_unified_concept assignment ON assignment.book_row_id=card.book_row_id
JOIN scope ON scope.concept_id=assignment.concept_id
WHERE (:downloaded_only=0 OR card.downloaded)
  AND (
      :file_types = 0
      OR ((:file_types & 1) != 0 AND LOWER(book.format) NOT IN ('pdf','m4b'))
      OR ((:file_types & 2) != 0 AND LOWER(book.format) = 'pdf')
      OR ((:file_types & 4) != 0 AND LOWER(book.format) = 'm4b')
  )
  AND (
      :languages = ''
      OR EXISTS(
          SELECT 1 FROM book_language
          WHERE book_language.book_row_id=book.row_id
            AND (
                instr(:languages, ',' || LOWER(book_language.language_tag) || ',') > 0
                OR (
                    instr(LOWER(book_language.language_tag), '-') > 0
                    AND instr(
                        :languages,
                        ',' || substr(LOWER(book_language.language_tag), 1, instr(LOWER(book_language.language_tag), '-') - 1) || ','
                    ) > 0
                )
            )
      )
  )
  AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL)
  AND (
      (:normalized_query = '' AND (:include_descendants=1 OR scope.path=:subject_path))
      OR (
          :normalized_query != ''
          AND (
              instr(normalize_search_text(COALESCE(card.title,'') || COALESCE(': ' || card.subtitle,'')), :normalized_query) > 0
              OR instr(normalize_search_text(COALESCE(card.author, '')), :normalized_query) > 0
          )
      )
  )
ORDER BY
    CASE WHEN :book_sort=1 THEN (
        SELECT MAX(reading.changed_at)
        FROM sync_state_version AS reading
        WHERE reading.state_kind='reading_position' AND reading.state_key=card.content_hash
    ) END DESC,
    normalize_search_text(COALESCE(card.title, '')), card.title, card.content_hash;

-- name: search_book_cards_by_title?
-- param: normalized_query: &str
-- param: downloaded_only: i32
SELECT content_hash, title, author, description, progress, downloaded, download_requested,
       format_category, audiobook_duration_ms, audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=book_cards.book_row_id), 0), subtitle
FROM book_cards
WHERE instr(normalize_search_text(COALESCE(title,'') || COALESCE(': ' || subtitle,'')), :normalized_query) > 0
  AND (:downloaded_only = 0 OR downloaded)
ORDER BY normalize_search_text(COALESCE(title, '')), content_hash;

-- name: search_book_cards_by_author?
-- param: normalized_query: &str
-- param: downloaded_only: i32
SELECT content_hash, title, author, description, progress, downloaded, download_requested,
       format_category, audiobook_duration_ms, audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=book_cards.book_row_id), 0), subtitle
FROM book_cards
WHERE instr(normalize_search_text(COALESCE(author, '')), :normalized_query) > 0
  AND instr(normalize_search_text(COALESCE(title,'') || COALESCE(': ' || subtitle,'')), :normalized_query) = 0
  AND (:downloaded_only = 0 OR downloaded)
ORDER BY normalize_search_text(COALESCE(title, '')), content_hash;

-- name: get_book_cards_in_dir?
-- param: dir_id: &str
-- param: downloaded_only: i32
-- param: normalized_query: &str
-- param: file_types: i32
-- param: book_sort: i32
-- param: languages: &str
WITH RECURSIVE scope(id) AS (
    SELECT id FROM dir WHERE id=:dir_id AND deleted_at IS NULL
    UNION
    SELECT child.id
    FROM dir AS child
    JOIN scope AS parent ON child.parent_id=parent.id
    WHERE child.id != parent.id AND child.deleted_at IS NULL
), matching_placement(book_row_id, source_directory) AS (
    SELECT placement.book_row_id, MIN(placement.dir_id)
    FROM book_dir placement
    JOIN scope ON scope.id=placement.dir_id
    WHERE placement.deleted_at IS NULL
      AND (:normalized_query != '' OR placement.dir_id=:dir_id)
    GROUP BY placement.book_row_id
)
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE(book.added_at, 0), matching_placement.source_directory, card.subtitle
FROM book_cards card
JOIN book ON book.row_id=card.book_row_id
JOIN matching_placement ON matching_placement.book_row_id=card.book_row_id
WHERE (:downloaded_only = 0 OR card.downloaded)
  AND (
      :file_types = 0
      OR ((:file_types & 1) != 0 AND LOWER(book.format) NOT IN ('pdf','m4b'))
      OR ((:file_types & 2) != 0 AND LOWER(book.format) = 'pdf')
      OR ((:file_types & 4) != 0 AND LOWER(book.format) = 'm4b')
  )
  AND (
      :languages = ''
      OR EXISTS(
          SELECT 1 FROM book_language
          WHERE book_language.book_row_id=book.row_id
            AND (
                instr(:languages, ',' || LOWER(book_language.language_tag) || ',') > 0
                OR (
                    instr(LOWER(book_language.language_tag), '-') > 0
                    AND instr(
                        :languages,
                        ',' || substr(LOWER(book_language.language_tag), 1, instr(LOWER(book_language.language_tag), '-') - 1) || ','
                    ) > 0
                )
            )
      )
  )
  AND (
      :normalized_query = ''
      OR instr(normalize_search_text(COALESCE(card.title,'') || COALESCE(': ' || card.subtitle,'')), :normalized_query) > 0
      OR instr(normalize_search_text(COALESCE(card.author, '')), :normalized_query) > 0
  )
ORDER BY
    CASE WHEN :book_sort=1 THEN (
        SELECT MAX(reading.changed_at)
        FROM sync_state_version AS reading
        WHERE reading.state_kind='reading_position' AND reading.state_key=card.content_hash
    ) END DESC,
    normalize_search_text(COALESCE(card.title, '')), card.title, card.content_hash;

-- name: get_book_cards_by_author?
-- param: stable_id: &str
-- param: downloaded_only: i32
-- DISTINCT because `author_credit` is keyed by position: one book crediting the
-- same identity twice — two spellings that normalized together — would
-- otherwise appear twice in the grid.
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0), card.subtitle
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
    SELECT path,concept_id FROM curated.unified_concept_route WHERE path=:subject_path
    UNION ALL
    SELECT child.path,child.concept_id
    FROM routes parent
    JOIN curated.unified_concept_route child ON child.parent_path=parent.path
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
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id=card.book_row_id), 0), card.subtitle
FROM book_cards card
JOIN book_external_entity reference ON reference.book_row_id=card.book_row_id
WHERE reference.wikidata_id=:wikidata_id
  AND (:downloaded_only=0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

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

-- name: get_author_book_cards?
-- param: downloaded_only: i32
-- param: limit: i32
-- The books an author's shelf carries, most recently read first, then by title,
-- capped per author. Whole cards rather than cover hashes: the shelf is the
-- same carousel Home draws, so it needs everything a book card renders. One
-- statement for the whole index rather than a correlated lookup per row, so the
-- cost does not scale with the list.
WITH credited AS (
    -- DISTINCT, not GROUP BY: one book credited twice to the same identity —
    -- two spellings that resolved together — is still one book, and every
    -- column here comes from that one card row.
    SELECT DISTINCT
           author_identity.stable_id AS author_id,
           card.book_row_id AS book_row_id,
           card.content_hash, card.title, card.author, card.description, card.progress,
           card.downloaded, card.download_requested, card.format_category,
           card.audiobook_duration_ms, card.audiobook_chapter_count,
           (SELECT reading.changed_at FROM sync_state_version reading
             WHERE reading.state_kind = 'reading_position'
               AND reading.state_key = card.content_hash) AS read_at, card.subtitle
    FROM author_identity
    JOIN author_credit ON author_credit.author_identity_id = author_identity.id
    JOIN book_cards card ON card.book_row_id = author_credit.book_row_id
    WHERE (:downloaded_only = 0 OR card.downloaded)
)
SELECT content_hash, title, author, description, progress,
       downloaded, download_requested, format_category,
       audiobook_duration_ms, audiobook_chapter_count,
       COALESCE((SELECT added_at FROM book WHERE row_id = book_row_id), 0),
       -- The author index is placement-neutral, so a card carries no folder.
       NULL,
       author_id, subtitle
FROM (
    SELECT *,
           ROW_NUMBER() OVER (
               PARTITION BY author_id
               ORDER BY COALESCE(read_at, 0) DESC, LOWER(COALESCE(title, '')), content_hash
           ) AS position
    FROM credited
)
WHERE position <= :limit
ORDER BY author_id, position;

-- name: get_author_subjects?
-- param: downloaded_only: i32
-- param: per_author: i32
-- The subjects an author writes about, most-credited first, capped per author.
-- Directly assigned concepts rather than the browse projection's ancestors: the
-- projection carries every parent up to "Subject", which would label every
-- author with the same handful of top-level words.
--
-- One chip per label, because a chip states a word rather than a place in the
-- tree — but the word is qualified by a parent, since "Communities" or
-- "20th century" says nothing alone. A label can sit at several routes, so the
-- parent quoted is the one accounting for most of this author's books under
-- that word; a top-level label has no parent and is stated bare.
WITH credited AS (
    SELECT DISTINCT
           author_identity.stable_id AS author_id,
           assignment.concept_id AS concept_id,
           book.row_id AS book_row_id
    FROM author_identity
    JOIN author_credit ON author_credit.author_identity_id = author_identity.id
    JOIN book ON book.row_id = author_credit.book_row_id
    JOIN book_unified_concept AS assignment ON assignment.book_row_id = book.row_id
    WHERE book.deleted_at IS NULL
      AND book.hidden_at IS NULL
      AND TRIM(author_credit.credited_name) != ''
      AND EXISTS (
          SELECT 1 FROM book_dir placement
          WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
      )
      AND (:downloaded_only = 0 OR EXISTS (
          SELECT 1 FROM book_dir available
          WHERE available.book_row_id = book.row_id
            AND available.deleted_at IS NULL
            AND available.is_downloaded = 1
      ))
),
routed AS (
    SELECT credited.author_id AS author_id,
           route.label AS label,
           route.parent_path AS parent_path,
           COUNT(DISTINCT credited.book_row_id) AS book_count
    FROM credited
    JOIN curated.unified_concept_route AS route ON route.concept_id = credited.concept_id
    WHERE TRIM(COALESCE(route.label, '')) != ''
    GROUP BY credited.author_id, route.label, route.parent_path
),
counted AS (
    SELECT author_id, label, parent_path,
           SUM(book_count) OVER (PARTITION BY author_id, label) AS label_count,
           ROW_NUMBER() OVER (PARTITION BY author_id, label ORDER BY book_count DESC, parent_path) AS route_position
    FROM routed
)
SELECT author_id, label, parent_label
FROM (
    SELECT counted.author_id AS author_id,
           counted.label AS label,
           COALESCE(parent.label, '') AS parent_label,
           ROW_NUMBER() OVER (
               PARTITION BY counted.author_id
               ORDER BY counted.label_count DESC, normalize_search_text(counted.label), counted.label
           ) AS position
    FROM counted
    LEFT JOIN curated.unified_concept_route AS parent ON parent.path = counted.parent_path
    WHERE counted.route_position = 1
)
WHERE position <= :per_author
ORDER BY author_id, position;

-- name: get_recently_read_cards?
-- param: limit: i32
-- param: downloaded_only: i32
-- param: finished_threshold: f64
-- The limit applies to books in progress and to finished books separately.
-- A single LIMIT over the whole list let a run of finished books crowd out
-- the ones still being read, which is the list Home exists to show; ranking
-- within each group spends the limit twice in one statement rather than
-- asking for the two lists in two round trips.
WITH read_cards AS (
  SELECT card.content_hash, card.title, card.author, card.description, card.progress,
         card.downloaded, card.download_requested, card.format_category,
         card.audiobook_duration_ms, card.audiobook_chapter_count,
         COALESCE(card.progress, 0) >= :finished_threshold AS finished,
         reading_version.changed_at AS changed_at, card.subtitle
  FROM book_cards card
  JOIN sync_state_version reading_version
    ON reading_version.state_kind = 'reading_position'
   AND reading_version.state_key = card.content_hash
  WHERE (:downloaded_only = 0 OR card.downloaded)
)
SELECT content_hash, title, author, description, progress,
       downloaded, download_requested, format_category,
       audiobook_duration_ms, audiobook_chapter_count, finished, subtitle
FROM (
    SELECT *, ROW_NUMBER() OVER (
        PARTITION BY finished
        ORDER BY changed_at DESC, normalize_search_text(COALESCE(title, '')), title, content_hash
    ) AS position
    FROM read_cards
)
WHERE position <= :limit
ORDER BY finished, position;

-- name: get_most_progress_cards?
-- param: limit: i32
-- param: downloaded_only: i32
-- param: finished_threshold: f64
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count, card.subtitle
FROM book_cards card
WHERE (:downloaded_only = 0 OR card.downloaded)
  AND COALESCE(card.progress, 0) > 0
  AND COALESCE(card.progress, 0) < :finished_threshold
ORDER BY card.progress DESC,
         normalize_search_text(COALESCE(card.title, '')), card.title, card.content_hash
LIMIT :limit;

-- name: get_recently_added_cards?
-- param: limit: i32
-- param: downloaded_only: i32
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count, card.subtitle
FROM book_cards card
JOIN book ON book.row_id = card.book_row_id
WHERE (:downloaded_only = 0 OR card.downloaded)
ORDER BY COALESCE(book.added_at, 0) DESC, LOWER(COALESCE(card.title, '')), card.content_hash
LIMIT :limit;

-- name: count_all_books?
-- Counts only books with a live placement, matching what the carousels display.
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL
  AND hidden_at IS NULL
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_row_id = book.row_id AND bd.deleted_at IS NULL);

-- name: rebuild_folder_book_ownership &
DELETE FROM folder_book_ownership;
INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
WITH RECURSIVE ancestry(descendant_id, ancestor_id) AS (
    SELECT id, id
    FROM dir
    WHERE deleted_at IS NULL

    UNION ALL

    SELECT ancestry.descendant_id, parent.id
    FROM ancestry
    JOIN dir AS current ON current.id=ancestry.ancestor_id
    JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
    WHERE current.id != current.parent_id
)
SELECT
    ancestry.ancestor_id,
    placement.book_row_id,
    COUNT(*),
    SUM(CASE WHEN placement.is_downloaded=1 THEN 1 ELSE 0 END)
FROM book_dir AS placement
JOIN ancestry ON ancestry.descendant_id=placement.dir_id
WHERE placement.deleted_at IS NULL
GROUP BY ancestry.ancestor_id, placement.book_row_id;


-- name: rebuild_book_browse_dimensions &
DELETE FROM book_browse_format;
INSERT INTO book_browse_format(book_row_id, format_category)
SELECT row_id, CASE lower(format) WHEN 'pdf' THEN 2 WHEN 'm4b' THEN 4 ELSE 1 END FROM book;
DELETE FROM book_browse_language;
INSERT INTO book_browse_language(book_row_id, base_language)
SELECT DISTINCT book_row_id,
    CASE
        WHEN instr(replace(lower(trim(language_tag)), '_', '-'), '-') > 0
            THEN substr(replace(lower(trim(language_tag)), '_', '-'), 1, instr(replace(lower(trim(language_tag)), '_', '-'), '-') - 1)
        ELSE replace(lower(trim(language_tag)), '_', '-')
    END
FROM book_language
WHERE trim(language_tag) != '';

-- name: rebuild_subject_book_ownership &
DELETE FROM subject_book_ownership;
INSERT INTO subject_book_ownership(subject_path, book_row_id, assignment_count)
WITH RECURSIVE route_ancestors(origin_path, path, parent_path) AS (
    SELECT DISTINCT route.path, route.path, route.parent_path FROM book_unified_concept assignment
    JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id
    UNION ALL
    SELECT ancestors.origin_path, parent.path, parent.parent_path
    FROM route_ancestors AS ancestors
    JOIN curated.unified_concept_route AS parent ON parent.path=ancestors.parent_path
    WHERE ancestors.path != ancestors.parent_path
)
SELECT ancestors.path, assignment.book_row_id, COUNT(*)
FROM book_unified_concept AS assignment
JOIN curated.unified_concept_route AS route ON route.concept_id=assignment.concept_id
JOIN route_ancestors AS ancestors ON ancestors.origin_path=route.path
GROUP BY ancestors.path, assignment.book_row_id;
INSERT INTO subject_book_ownership(subject_path, book_row_id, assignment_count)
SELECT 'Subject', book_row_id, COUNT(*)
FROM book_unified_concept
GROUP BY book_row_id
ON CONFLICT(subject_path, book_row_id) DO UPDATE SET assignment_count=excluded.assignment_count;

-- name: book_detail_summary_select?
-- param: content_hash: &str
SELECT content_hash,title,author,description,progress,downloaded,download_requested,format_category,audiobook_duration_ms,audiobook_chapter_count,added_at,NULL,subtitle FROM book_cards WHERE content_hash=:content_hash AND EXISTS (SELECT 1 FROM book_dir WHERE book_row_id=book_cards.book_row_id AND deleted_at IS NULL);

-- name: book_detail_summary_select_2?
-- param: content_hash: &str
SELECT dir_id FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL ORDER BY dir_id LIMIT 1;

-- name: book_detail_summary_select_3?
-- param: content_hash: &str
SELECT identity.stable_id,credit.credited_name FROM author_credit credit JOIN author_identity identity ON identity.id=credit.author_identity_id WHERE credit.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) ORDER BY credit.position;

-- name: book_detail_summary_select_4?
-- param: content_hash: &str
-- param: narrator_role: Vec<u8>
-- param: translator_role: Vec<u8>
SELECT identity.stable_id,contributor.name,contributor.role FROM book_contributor contributor JOIN author_identity identity ON identity.id=contributor.author_identity_id WHERE contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND contributor.role IN (:narrator_role,:translator_role) ORDER BY contributor.position;

-- name: book_detail_summary_select_5?
-- param: content_hash: &str
SELECT DISTINCT route.path FROM book_unified_concept assignment JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id WHERE assignment.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) ORDER BY route.path COLLATE NOCASE,route.path;

-- name: external_kind_counts_select?
-- param: kind: &str
SELECT COUNT(DISTINCT reference.book_row_id),COUNT(DISTINCT CASE WHEN card.downloaded THEN reference.book_row_id END)
         FROM book_external_entity reference JOIN book_cards card ON card.book_row_id=reference.book_row_id WHERE reference.kind=:kind;

-- name: external_kind_hashes_select?
-- param: kind: &str
SELECT DISTINCT book.content_hash FROM book_external_entity reference JOIN book ON book.row_id=reference.book_row_id
         WHERE reference.kind=:kind AND book.deleted_at IS NULL AND book.hidden_at IS NULL ORDER BY book.content_hash;

-- name: get_external_entity_content_hashes_select?
-- param: wikidata_id: &str
SELECT DISTINCT book.content_hash FROM book_external_entity reference JOIN book ON book.row_id=reference.book_row_id
         WHERE reference.wikidata_id=:wikidata_id AND book.deleted_at IS NULL AND book.hidden_at IS NULL ORDER BY book.content_hash;

-- name: group_folder_children_with?
-- param: directory_ids: &str
-- param: file_types: i32
-- param: languages: &str
-- param: chip_sort: i32
-- param: downloaded_only: bool
WITH RECURSIVE candidates(id,parent_id,name) AS (
            SELECT d.id,d.parent_id,d.name FROM json_each(:directory_ids) selected
            JOIN dir parent ON parent.id=selected.value
            JOIN dir d ON d.parent_id=parent.id AND d.id!=parent.id
            WHERE parent.deleted_at IS NULL AND d.deleted_at IS NULL AND d.name NOT LIKE '.%'
              AND NOT EXISTS(SELECT 1 FROM book_dir p JOIN book b ON b.row_id=p.book_row_id
                  WHERE p.dir_id=parent.id AND p.deleted_at IS NULL AND b.deleted_at IS NULL AND b.hidden_at IS NULL)
        ), tree(folder_id,id) AS (
            SELECT id,id FROM candidates
            UNION ALL
            SELECT t.folder_id,d.id FROM tree t JOIN dir d ON d.parent_id=t.id AND d.id!=t.id
            WHERE d.deleted_at IS NULL AND d.name NOT LIKE '.%'
        ), matched AS (
            SELECT b.row_id FROM book b WHERE b.deleted_at IS NULL AND b.hidden_at IS NULL
            AND (:file_types=0 OR ((:file_types & 1)!=0 AND lower(b.format) NOT IN ('pdf','m4b'))
                OR ((:file_types & 2)!=0 AND lower(b.format)='pdf') OR ((:file_types & 4)!=0 AND lower(b.format)='m4b'))
            AND (:languages='' OR EXISTS(SELECT 1 FROM book_language l WHERE l.book_row_id=b.row_id
                AND (instr(:languages,',' || lower(l.language_tag) || ',')>0
                    OR (instr(l.language_tag,'-')>0 AND instr(:languages,',' || substr(lower(l.language_tag),1,instr(l.language_tag,'-')-1) || ',')>0))))
        ) SELECT c.parent_id,c.id,c.name,count(DISTINCT b.row_id) AS total,
            count(DISTINCT CASE WHEN p.is_downloaded THEN b.row_id END) AS downloaded
        FROM candidates c JOIN tree t ON t.folder_id=c.id
        LEFT JOIN book_dir p ON p.dir_id=t.id AND p.deleted_at IS NULL
        LEFT JOIN matched b ON b.row_id=p.book_row_id
        GROUP BY c.parent_id,c.id,c.name
        ORDER BY c.parent_id,CASE WHEN :chip_sort=1 THEN CASE WHEN :downloaded_only THEN downloaded ELSE total END END DESC,
            normalize_search_text(c.name),c.name,c.id;

-- name: library_format_counts_select?
SELECT
             CASE lower(book.format)
                 WHEN 'pdf' THEN 2
                 WHEN 'm4b' THEN 3
                 ELSE 1
             END AS format_category,
             COUNT(*)
         FROM book
         WHERE book.deleted_at IS NULL
           AND book.hidden_at IS NULL
           AND EXISTS (
               SELECT 1
               FROM book_dir AS placement
               WHERE placement.book_row_id=book.row_id
                 AND placement.deleted_at IS NULL
           )
         GROUP BY format_category
         ORDER BY format_category;

-- name: library_language_counts_with?
WITH visible_book_languages AS (
             SELECT DISTINCT
                 language.book_row_id,
                 CASE
                     WHEN instr(replace(lower(trim(language.language_tag)), '_', '-'), '-') > 0
                         THEN substr(
                             replace(lower(trim(language.language_tag)), '_', '-'),
                             1,
                             instr(replace(lower(trim(language.language_tag)), '_', '-'), '-') - 1
                         )
                     ELSE replace(lower(trim(language.language_tag)), '_', '-')
                 END AS base_language
             FROM book_language AS language
             JOIN book ON book.row_id=language.book_row_id
             WHERE book.deleted_at IS NULL
               AND book.hidden_at IS NULL
               AND trim(language.language_tag) != ''
               AND EXISTS (
                   SELECT 1
                   FROM book_dir AS placement
                   WHERE placement.book_row_id=book.row_id
                     AND placement.deleted_at IS NULL
               )
         )
         SELECT base_language, COUNT(*)
         FROM visible_book_languages
         WHERE base_language != ''
         GROUP BY base_language
         ORDER BY base_language;

-- name: load_select?
SELECT book.content_hash,book.format,MAX(placement.is_downloaded)
             FROM book JOIN book_dir placement ON placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
             WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
             GROUP BY book.row_id,book.content_hash,book.format;

-- name: related_books?
-- param: author_ids: &str
-- param: content_hash: &str
SELECT DISTINCT card.content_hash,card.title,card.author,card.description,card.progress,card.downloaded,card.download_requested,card.format_category,card.audiobook_duration_ms,card.audiobook_chapter_count,card.added_at,NULL,card.subtitle FROM book_cards card JOIN author_credit credit ON credit.book_row_id=card.book_row_id JOIN author_identity identity ON identity.id=credit.author_identity_id WHERE identity.stable_id IN (SELECT value FROM json_each(:author_ids)) AND card.content_hash<>:content_hash AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=card.book_row_id AND placement.deleted_at IS NULL) ORDER BY LOWER(card.title),card.title,card.content_hash LIMIT 12;

-- name: subject_facet_counts_scoped_select?
SELECT dirty FROM browse_projection_state WHERE projection='subject_ownership';

-- name: subject_facet_counts_scoped_update_2!
UPDATE main.browse_projection_state SET dirty=dirty WHERE projection='subject_ownership';

-- name: book_ownership!
-- param: book_row_id: i64
INSERT INTO subject_book_ownership(subject_path,book_row_id,assignment_count)
WITH RECURSIVE routes(origin,path,parent_path) AS (
    SELECT r.path,r.path,r.parent_path FROM book_unified_concept a
    JOIN curated.unified_concept_route r ON r.concept_id=a.concept_id WHERE a.book_row_id=:book_row_id
    UNION ALL
    SELECT child.origin,r.path,r.parent_path FROM routes child
    JOIN curated.unified_concept_route r ON r.path=child.parent_path WHERE child.path!=child.parent_path
) SELECT path,:book_row_id,COUNT(*) FROM routes GROUP BY path;

-- name: book_routes?
-- param: book_row_id: i64
WITH RECURSIVE routes(path,parent_path,concept_id,label,direct) AS (
    SELECT r.path,r.parent_path,r.concept_id,r.label,1 FROM book_unified_concept a
    JOIN book b ON b.row_id=a.book_row_id JOIN curated.unified_concept_route r ON r.concept_id=a.concept_id
    WHERE b.row_id=:book_row_id AND b.deleted_at IS NULL AND b.hidden_at IS NULL
      AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=b.row_id AND p.deleted_at IS NULL)
    UNION
    SELECT r.path,r.parent_path,r.concept_id,r.label,0 FROM routes child
    JOIN curated.unified_concept_route r ON r.path=child.parent_path WHERE child.path!=child.parent_path
) SELECT path,parent_path,concept_id,label,MAX(direct) FROM routes GROUP BY path;

-- name: build_with?
-- param: matched_hashes: &str
WITH RECURSIVE direct(concept_id,hash) AS (
                SELECT assignment.concept_id,book.content_hash FROM json_each(:matched_hashes) selected
                JOIN book ON book.content_hash=selected.value
                JOIN book_unified_concept assignment ON assignment.book_row_id=book.row_id
            ), used(path,parent_path,concept_id,label) AS (
                SELECT route.path,route.parent_path,route.concept_id,route.label
                FROM direct JOIN library_subject route ON route.concept_id=direct.concept_id
                UNION
                SELECT parent.path,parent.parent_path,parent.concept_id,parent.label
                FROM used child JOIN library_subject parent ON parent.path=child.parent_path
                WHERE child.path!=child.parent_path
            )
            SELECT used.path,used.parent_path,used.label,direct.hash
            FROM used
            LEFT JOIN direct ON direct.concept_id=used.concept_id;

-- name: download_flags?
-- param: hashes: &str
SELECT CAST(selected.key AS INTEGER),card.downloaded,card.download_requested
FROM json_each(:hashes) selected JOIN book_cards card ON card.content_hash=selected.value;

-- name: downloaded_books?
SELECT DISTINCT book_row_id FROM book_dir WHERE deleted_at IS NULL AND is_downloaded=1 ORDER BY book_row_id;

-- name: ensure_projection_update_2!
UPDATE browse_projection_state SET dirty=0 WHERE projection='subject_ownership';

-- name: load_cards_all?
-- param: location: Option<&str>
-- param: downloaded_only: i32
-- param: file_types: i32
-- param: languages: &str
-- param: sort: i32
SELECT card.content_hash,card.title,card.author,card.description,card.progress,
card.downloaded,card.download_requested,card.format_category,card.audiobook_duration_ms,card.audiobook_chapter_count,COALESCE(book.added_at,0),card.subtitle FROM book_cards card JOIN book ON book.row_id=card.book_row_id WHERE :location IS NULL
         AND EXISTS(SELECT 1 FROM book_unified_concept a JOIN library_subject r ON r.concept_id=a.concept_id WHERE a.book_row_id=card.book_row_id)
    AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=card.book_row_id AND p.deleted_at IS NULL)
    AND (:downloaded_only=0 OR card.downloaded)
    AND (:file_types=0 OR EXISTS(SELECT 1 FROM book_browse_format f WHERE f.book_row_id=card.book_row_id AND (f.format_category & :file_types)!=0))
    AND (:languages='' OR EXISTS(SELECT 1 FROM book_language l WHERE l.book_row_id=card.book_row_id AND
        (instr(:languages,',' || lower(l.language_tag) || ',')>0 OR
         (instr(l.language_tag,'-')>0 AND instr(:languages,',' || substr(lower(l.language_tag),1,instr(l.language_tag,'-')-1) || ',')>0))))
    ORDER BY CASE WHEN :sort=1 THEN (SELECT MAX(changed_at) FROM sync_state_version WHERE state_kind='reading_position' AND state_key=card.content_hash) END DESC,
    normalize_search_text(COALESCE(card.title,'')),card.title,card.content_hash;

-- name: load_cards_location?
-- param: location: Option<&str>
-- param: downloaded_only: i32
-- param: file_types: i32
-- param: languages: &str
-- param: sort: i32
SELECT card.content_hash,card.title,card.author,card.description,card.progress,
card.downloaded,card.download_requested,card.format_category,card.audiobook_duration_ms,card.audiobook_chapter_count,COALESCE(book.added_at,0),card.subtitle FROM subject_browse_book shown JOIN book_cards card ON card.book_row_id=shown.book_row_id
         JOIN book ON book.row_id=card.book_row_id WHERE shown.location=:location
    AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=card.book_row_id AND p.deleted_at IS NULL)
    AND (:downloaded_only=0 OR card.downloaded)
    AND (:file_types=0 OR EXISTS(SELECT 1 FROM book_browse_format f WHERE f.book_row_id=card.book_row_id AND (f.format_category & :file_types)!=0))
    AND (:languages='' OR EXISTS(SELECT 1 FROM book_language l WHERE l.book_row_id=card.book_row_id AND
        (instr(:languages,',' || lower(l.language_tag) || ',')>0 OR
         (instr(l.language_tag,'-')>0 AND instr(:languages,',' || substr(lower(l.language_tag),1,instr(l.language_tag,'-')-1) || ',')>0))))
    ORDER BY CASE WHEN :sort=1 THEN (SELECT MAX(changed_at) FROM sync_state_version WHERE state_kind='reading_position' AND state_key=card.content_hash) END DESC,
    normalize_search_text(COALESCE(card.title,'')),card.title,card.content_hash;

-- name: stored_children?
-- param: location: &str
-- param: chip_sort: i32
SELECT r.path,r.label,c.book_count,
    (SELECT COUNT(*) FROM subject_browse_member m WHERE m.path=r.path
        AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=m.book_row_id AND p.deleted_at IS NULL AND p.is_downloaded=1))
FROM subject_browse_child edge JOIN library_subject r ON r.path=edge.path
JOIN subject_browse_count c ON c.path=r.path WHERE edge.location=:location
ORDER BY CASE WHEN :chip_sort=1 THEN c.book_count END DESC,normalize_search_text(r.label),r.label,r.path;

-- name: subject_contents_view_select_2?
SELECT replica_id FROM sync_metadata WHERE singleton = 1;

-- name: subject_contents_view_select_3?
SELECT file FROM pragma_database_list WHERE name='main';

-- name: subject_contents_view_select_4?
SELECT revision FROM subject_browse_state WHERE id=1;

-- name: update_delete!
-- param: book_row_id: i64
DELETE FROM subject_browse_member WHERE book_row_id=:book_row_id;

-- name: update_delete_2!
-- param: book_row_id: i64
DELETE FROM subject_book_ownership WHERE book_row_id=:book_row_id;

-- name: update_delete_3!
-- param: location: &str
DELETE FROM subject_browse_raw_child WHERE location=:location;

-- name: update_delete_4!
-- param: location: &str
DELETE FROM subject_browse_raw_book WHERE location=:location;

-- name: update_delete_5!
-- param: location: &str
DELETE FROM subject_browse_child WHERE location=:location;

-- name: update_delete_6!
-- param: location: &str
DELETE FROM subject_browse_book WHERE location=:location;

-- name: update_delete_7!
-- param: path: &str
DELETE FROM library_subject WHERE path=:path AND NOT EXISTS(SELECT 1 FROM subject_browse_member WHERE path=:path);

-- name: update_delete_8&
DELETE FROM subject_browse_dirty_book;
        UPDATE subject_browse_state SET built_revision=tree_revision WHERE id=1;
        UPDATE browse_projection_state SET dirty=0 WHERE projection='subject_ownership';

-- name: update_insert&
INSERT OR IGNORE INTO subject_browse_dirty_book SELECT row_id FROM book;
            DELETE FROM library_subject; DELETE FROM subject_browse_member;
            DELETE FROM subject_browse_child; DELETE FROM subject_browse_book; DELETE FROM subject_browse_count;
            DELETE FROM subject_browse_raw_child; DELETE FROM subject_browse_raw_book;
            DELETE FROM subject_book_ownership;

-- name: update_insert_2!
-- param: path: &str
-- param: parent_path: &str
-- param: concept_id: i64
-- param: label: &str
INSERT OR IGNORE INTO library_subject SELECT :path,:parent_path,:concept_id,:label WHERE :path!='';

-- name: update_insert_3!
-- param: path: &str
-- param: book_row_id: i64
-- param: direct: i64
INSERT INTO subject_browse_member VALUES(:path,:book_row_id,:direct);

-- name: update_insert_4!
-- param: book_row_id: i64
INSERT INTO subject_book_ownership SELECT 'Subject',book_row_id,COUNT(*) FROM book_unified_concept WHERE book_row_id=:book_row_id GROUP BY book_row_id ON CONFLICT(subject_path,book_row_id) DO UPDATE SET assignment_count=excluded.assignment_count;

-- name: update_insert_5!
-- param: location: &str
-- param: path: &str
INSERT INTO subject_browse_raw_child VALUES(:location,:path);

-- name: update_insert_6!
-- param: location: &str
-- param: book_row_id: i64
INSERT INTO subject_browse_raw_book VALUES(:location,:book_row_id);

-- name: update_insert_7!
-- param: location: &str
-- param: path: &str
INSERT INTO subject_browse_child VALUES(:location,:path);

-- name: update_insert_8!
-- param: location: &str
-- param: book_row_id: i64
INSERT INTO subject_browse_book VALUES(:location,:book_row_id);

-- name: update_insert_9!
-- param: path: &str
INSERT INTO subject_browse_count SELECT :path,COUNT(*) FROM subject_browse_member WHERE path=:path ON CONFLICT(path) DO UPDATE SET book_count=excluded.book_count;

-- name: update_select?
-- param: path: &str
SELECT tree_revision,built_revision,
    EXISTS(SELECT 1 FROM subject_browse_member)
    AND NOT EXISTS(SELECT 1 FROM subject_browse_member WHERE path IN('',:path))
FROM subject_browse_state WHERE id=1;

-- name: update_select_2?
SELECT EXISTS(SELECT 1 FROM subject_browse_dirty_book);

-- name: update_select_3?
SELECT book_row_id FROM subject_browse_dirty_book;

-- name: update_select_4?
-- param: book_row_id: i64
SELECT path,direct FROM subject_browse_member WHERE book_row_id=:book_row_id;

-- name: update_select_6?
-- param: path: &str
SELECT book_row_id FROM subject_browse_member WHERE path=:path AND direct=1;

-- name: update_select_7?
-- param: parent_path: &str
SELECT r.path FROM library_subject r JOIN subject_browse_count c ON c.path=r.path WHERE r.parent_path=:parent_path AND r.path!=:parent_path AND c.book_count>0;

-- name: update_update!
UPDATE subject_browse_state SET built_revision=built_revision WHERE id=1;

-- name: schema_browse &
-- Rebuildable browse projections. The rebuild statements that maintain
-- them live with the browse module; only their tables are created here.
--
-- One row in folder_book_ownership means a book has at least one live placement
-- in this folder's live subtree. Multiplicities preserve exact distinct-book and
-- downloaded counts when the same book is placed in several descendant folders.
CREATE TABLE IF NOT EXISTS folder_book_ownership(
    folder_id                    TEXT NOT NULL REFERENCES dir(id) ON DELETE CASCADE,
    book_row_id                  INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    placement_count              INTEGER NOT NULL CHECK(placement_count > 0),
    downloaded_placement_count   INTEGER NOT NULL CHECK(downloaded_placement_count >= 0 AND downloaded_placement_count <= placement_count),
    PRIMARY KEY(folder_id, book_row_id)
);

-- Foreign-key cascades and book-scoped diagnostics need the reverse lookup.
CREATE INDEX IF NOT EXISTS idx_folder_book_ownership_book
ON folder_book_ownership(book_row_id, folder_id);


-- Rebuildable browse dimensions keep normalization and hierarchy projection
-- out of interactive facet queries.
CREATE TABLE IF NOT EXISTS book_browse_format(
    book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE,
    format_category INTEGER NOT NULL CHECK(format_category IN (1, 2, 4))
);

CREATE TABLE IF NOT EXISTS book_browse_language(
    book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    base_language TEXT NOT NULL,
    PRIMARY KEY(book_row_id, base_language)
);
CREATE INDEX IF NOT EXISTS idx_book_browse_language_value ON book_browse_language(base_language, book_row_id);

-- A count is retained because several assigned concepts can place one book
-- under the same subject ancestor.
CREATE TABLE IF NOT EXISTS subject_book_ownership(
    subject_path TEXT NOT NULL,
    book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    assignment_count INTEGER NOT NULL CHECK(assignment_count > 0),
    PRIMARY KEY(subject_path, book_row_id)
);
CREATE INDEX IF NOT EXISTS idx_subject_book_ownership_book ON subject_book_ownership(book_row_id, subject_path);

CREATE TABLE IF NOT EXISTS browse_projection_state(
    projection TEXT PRIMARY KEY,
    dirty INTEGER NOT NULL CHECK(dirty IN (0,1))
);
INSERT OR IGNORE INTO browse_projection_state(projection,dirty) VALUES('subject_ownership',0);

-- name: schema_subjects &
-- Subject projection, placement triggers, incremental maintenance, download state.
-- Local derived state only: never synchronized and never stored in the taxonomy.
CREATE TABLE library_subject(
    path TEXT PRIMARY KEY, parent_path TEXT NOT NULL, concept_id INTEGER NOT NULL, label TEXT NOT NULL
) WITHOUT ROWID;
CREATE INDEX library_subject_parent ON library_subject(parent_path);
CREATE INDEX library_subject_concept ON library_subject(concept_id);
CREATE TABLE subject_browse_child(
    location TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY(location,path)
) WITHOUT ROWID;
CREATE TABLE subject_browse_book(
    location TEXT NOT NULL, book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    PRIMARY KEY(location,book_row_id)
) WITHOUT ROWID;
CREATE INDEX subject_browse_book_book ON subject_browse_book(book_row_id);
CREATE TABLE subject_browse_count(
    path TEXT PRIMARY KEY, book_count INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE subject_browse_state(
    id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL, tree_revision INTEGER NOT NULL, built_revision INTEGER NOT NULL
);
INSERT INTO subject_browse_state VALUES(1,0,0,-1);
CREATE TRIGGER subject_browse_book_insert AFTER INSERT ON book
BEGIN UPDATE subject_browse_state SET revision=revision+1, tree_revision=tree_revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_update AFTER UPDATE OF content_hash,title,format,hidden_at,read_pos,read_progress,added_at,deleted_at ON book
WHEN NEW.content_hash IS NOT OLD.content_hash OR NEW.title IS NOT OLD.title OR NEW.format IS NOT OLD.format OR NEW.hidden_at IS NOT OLD.hidden_at OR NEW.read_pos IS NOT OLD.read_pos OR NEW.read_progress IS NOT OLD.read_progress OR NEW.added_at IS NOT OLD.added_at OR NEW.deleted_at IS NOT OLD.deleted_at
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_delete AFTER DELETE ON book
BEGIN UPDATE subject_browse_state SET revision=revision+1, tree_revision=tree_revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_dir_update AFTER UPDATE OF book_row_id,file_name,deleted_at,is_downloaded ON book_dir
WHEN NEW.book_row_id IS NOT OLD.book_row_id OR NEW.file_name IS NOT OLD.file_name OR NEW.deleted_at IS NOT OLD.deleted_at OR NEW.is_downloaded IS NOT OLD.is_downloaded
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_unified_concept_insert AFTER INSERT ON book_unified_concept
BEGIN UPDATE subject_browse_state SET revision=revision+1, tree_revision=tree_revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_unified_concept_update AFTER UPDATE OF book_row_id,concept_id ON book_unified_concept
WHEN NEW.book_row_id IS NOT OLD.book_row_id OR NEW.concept_id IS NOT OLD.concept_id
BEGIN UPDATE subject_browse_state SET revision=revision+1, tree_revision=tree_revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_unified_concept_delete AFTER DELETE ON book_unified_concept
BEGIN UPDATE subject_browse_state SET revision=revision+1, tree_revision=tree_revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_language_insert AFTER INSERT ON book_language
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_language_update AFTER UPDATE ON book_language
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_language_delete AFTER DELETE ON book_language
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_contributor_insert AFTER INSERT ON book_contributor
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_contributor_update AFTER UPDATE ON book_contributor
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_contributor_delete AFTER DELETE ON book_contributor
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_author_identity_insert AFTER INSERT ON author_identity
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_author_identity_update AFTER UPDATE ON author_identity
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_author_identity_delete AFTER DELETE ON author_identity
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_audiobook_metadata_insert AFTER INSERT ON audiobook_metadata
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_audiobook_metadata_update AFTER UPDATE ON audiobook_metadata
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_audiobook_metadata_delete AFTER DELETE ON audiobook_metadata
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
-- The card's chapter count is a navigation entry count, so the browse
-- projection follows book_toc as well as the duration beside it.
CREATE TRIGGER subject_browse_book_toc_insert AFTER INSERT ON book_toc
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_toc_update AFTER UPDATE ON book_toc
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_toc_delete AFTER DELETE ON book_toc
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_sync_state_version_insert AFTER INSERT ON sync_state_version
WHEN NEW.state_kind='reading_position'
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_sync_state_version_update AFTER UPDATE ON sync_state_version
WHEN NEW.state_kind='reading_position' OR OLD.state_kind='reading_position'
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_sync_state_version_delete AFTER DELETE ON sync_state_version
WHEN OLD.state_kind='reading_position'
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_download_requests_insert AFTER INSERT ON download_requests
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_download_requests_update AFTER UPDATE ON download_requests
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_download_requests_delete AFTER DELETE ON download_requests
BEGIN UPDATE subject_browse_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER subject_browse_book_visibility AFTER UPDATE OF hidden_at,deleted_at ON book
WHEN NEW.hidden_at IS NOT OLD.hidden_at OR NEW.deleted_at IS NOT OLD.deleted_at
BEGIN UPDATE subject_browse_state SET tree_revision=tree_revision+1 WHERE id=1; END;

-- A subject view depends on having a live copy, not its folder.
CREATE TRIGGER subject_browse_book_dir_insert AFTER INSERT ON book_dir
BEGIN
    UPDATE subject_browse_state SET revision=revision+1,
        tree_revision=tree_revision + CASE WHEN NEW.deleted_at IS NULL AND (
            NOT EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=NEW.book_row_id AND p.dir_id!=NEW.dir_id AND p.deleted_at IS NULL)
        ) THEN 1 ELSE 0 END WHERE id=1;
END;
CREATE TRIGGER subject_browse_book_dir_delete AFTER DELETE ON book_dir
BEGIN
    UPDATE subject_browse_state SET revision=revision+1,
        tree_revision=tree_revision + CASE WHEN OLD.deleted_at IS NULL AND (
            NOT EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=OLD.book_row_id AND p.deleted_at IS NULL)
        ) THEN 1 ELSE 0 END WHERE id=1;
END;
CREATE TRIGGER subject_browse_placement_membership AFTER UPDATE OF book_row_id,deleted_at ON book_dir
WHEN NEW.book_row_id IS NOT OLD.book_row_id
    OR ((NEW.deleted_at IS NULL) != (OLD.deleted_at IS NULL)
        AND NOT EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=NEW.book_row_id AND p.dir_id!=NEW.dir_id AND p.deleted_at IS NULL))
BEGIN UPDATE subject_browse_state SET tree_revision=tree_revision+1 WHERE id=1; END;

-- Dirty book IDs survive deletion so their old subject paths can be removed.
CREATE TABLE IF NOT EXISTS subject_browse_dirty_book(book_row_id INTEGER PRIMARY KEY);
CREATE TABLE IF NOT EXISTS subject_browse_member(
    path TEXT NOT NULL, book_row_id INTEGER NOT NULL, direct INTEGER NOT NULL,
    PRIMARY KEY(path,book_row_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS subject_browse_member_book ON subject_browse_member(book_row_id,path);
CREATE TABLE IF NOT EXISTS subject_browse_raw_child(location TEXT NOT NULL,path TEXT NOT NULL,PRIMARY KEY(location,path)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS subject_browse_raw_book(location TEXT NOT NULL,book_row_id INTEGER NOT NULL,PRIMARY KEY(location,book_row_id)) WITHOUT ROWID;
CREATE TRIGGER IF NOT EXISTS subject_dirty_book_insert AFTER INSERT ON book

BEGIN
    INSERT INTO subject_browse_dirty_book SELECT NEW.row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=NEW.row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_book_delete AFTER DELETE ON book

BEGIN
    INSERT INTO subject_browse_dirty_book SELECT OLD.row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=OLD.row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_book_visibility AFTER UPDATE OF hidden_at,deleted_at ON book
WHEN NEW.hidden_at IS NOT OLD.hidden_at OR NEW.deleted_at IS NOT OLD.deleted_at
BEGIN
    INSERT INTO subject_browse_dirty_book SELECT NEW.row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=NEW.row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_assignment_insert AFTER INSERT ON book_unified_concept

BEGIN
    INSERT INTO subject_browse_dirty_book SELECT NEW.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=NEW.book_row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_assignment_delete AFTER DELETE ON book_unified_concept

BEGIN
    INSERT INTO subject_browse_dirty_book SELECT OLD.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=OLD.book_row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_assignment_update AFTER UPDATE OF book_row_id,concept_id ON book_unified_concept
WHEN NEW.book_row_id IS NOT OLD.book_row_id OR NEW.concept_id IS NOT OLD.concept_id
BEGIN
    INSERT INTO subject_browse_dirty_book SELECT OLD.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=OLD.book_row_id);
    INSERT INTO subject_browse_dirty_book SELECT NEW.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=NEW.book_row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_placement_insert AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
BEGIN
    INSERT INTO subject_browse_dirty_book SELECT NEW.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=NEW.book_row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_placement_delete AFTER DELETE ON book_dir
WHEN OLD.deleted_at IS NULL
BEGIN
    INSERT INTO subject_browse_dirty_book SELECT OLD.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=OLD.book_row_id);
END;
CREATE TRIGGER IF NOT EXISTS subject_dirty_placement_update AFTER UPDATE OF book_row_id,deleted_at ON book_dir
WHEN NEW.book_row_id IS NOT OLD.book_row_id OR NEW.deleted_at IS NOT OLD.deleted_at
BEGIN
    INSERT INTO subject_browse_dirty_book SELECT OLD.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=OLD.book_row_id);
    INSERT INTO subject_browse_dirty_book SELECT NEW.book_row_id WHERE NOT EXISTS(SELECT 1 FROM subject_browse_dirty_book WHERE book_row_id=NEW.book_row_id);
END;
UPDATE subject_browse_state SET built_revision=-1 WHERE id=1;

-- name: schema_sync_triggers &
-- Triggers that stamp local writes into the synchronization outbox.
CREATE TRIGGER trig_book_browse_format_insert
AFTER INSERT ON book
BEGIN
    INSERT INTO book_browse_format(book_row_id,format_category)
    VALUES(NEW.row_id,CASE lower(NEW.format) WHEN 'pdf' THEN 2 WHEN 'm4b' THEN 4 ELSE 1 END)
    ON CONFLICT(book_row_id) DO UPDATE SET format_category=excluded.format_category;
END;

CREATE TRIGGER trig_book_browse_format_update
AFTER UPDATE OF format ON book
BEGIN
    INSERT INTO book_browse_format(book_row_id,format_category)
    VALUES(NEW.row_id,CASE lower(NEW.format) WHEN 'pdf' THEN 2 WHEN 'm4b' THEN 4 ELSE 1 END)
    ON CONFLICT(book_row_id) DO UPDATE SET format_category=excluded.format_category;
END;

CREATE TRIGGER trig_book_browse_language_insert
AFTER INSERT ON book_language
BEGIN
    DELETE FROM book_browse_language WHERE book_row_id=NEW.book_row_id;
    INSERT INTO book_browse_language(book_row_id,base_language)
    SELECT DISTINCT book_row_id,
        CASE WHEN instr(replace(lower(trim(language_tag)),'_','-'),'-')>0
            THEN substr(replace(lower(trim(language_tag)),'_','-'),1,instr(replace(lower(trim(language_tag)),'_','-'),'-')-1)
            ELSE replace(lower(trim(language_tag)),'_','-') END
    FROM book_language WHERE book_row_id=NEW.book_row_id AND trim(language_tag)!='';
END;

CREATE TRIGGER trig_book_browse_language_update
AFTER UPDATE OF language_tag,book_row_id ON book_language
BEGIN
    DELETE FROM book_browse_language WHERE book_row_id IN (OLD.book_row_id,NEW.book_row_id);
    INSERT OR IGNORE INTO book_browse_language(book_row_id,base_language)
    SELECT DISTINCT book_row_id,
        CASE WHEN instr(replace(lower(trim(language_tag)),'_','-'),'-')>0
            THEN substr(replace(lower(trim(language_tag)),'_','-'),1,instr(replace(lower(trim(language_tag)),'_','-'),'-')-1)
            ELSE replace(lower(trim(language_tag)),'_','-') END
    FROM book_language WHERE book_row_id IN (OLD.book_row_id,NEW.book_row_id) AND trim(language_tag)!='';
END;

CREATE TRIGGER trig_book_browse_language_delete
AFTER DELETE ON book_language
BEGIN
    DELETE FROM book_browse_language WHERE book_row_id=OLD.book_row_id;
    INSERT INTO book_browse_language(book_row_id,base_language)
    SELECT DISTINCT book_row_id,
        CASE WHEN instr(replace(lower(trim(language_tag)),'_','-'),'-')>0
            THEN substr(replace(lower(trim(language_tag)),'_','-'),1,instr(replace(lower(trim(language_tag)),'_','-'),'-')-1)
            ELSE replace(lower(trim(language_tag)),'_','-') END
    FROM book_language WHERE book_row_id=OLD.book_row_id AND trim(language_tag)!='';
END;

-- Assignment writes only invalidate counts. Browsing rebuilds them once per
-- batch, using the final shared taxonomy, instead of walking all routes for
-- every single assignment inserted during an import.
CREATE TRIGGER trig_subject_book_ownership_insert
AFTER INSERT ON book_unified_concept
BEGIN UPDATE browse_projection_state SET dirty=1 WHERE projection='subject_ownership'; END;
CREATE TRIGGER trig_subject_book_ownership_delete
AFTER DELETE ON book_unified_concept
BEGIN UPDATE browse_projection_state SET dirty=1 WHERE projection='subject_ownership'; END;

-- Every producer passes typed SQL values through the registered Rust encoder.
-- Invalid protocol values abort the surrounding transaction; valid values are
-- stored as versioned bincode without an intermediate JSON representation.
CREATE TRIGGER trig_dir_state_insert
AFTER INSERT ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000'
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_name', NEW.id,
        bokheim_mutation('directory_name', NEW.id, NEW.name),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_parent', NEW.id,
        bokheim_mutation('directory_parent', NEW.id, NEW.parent_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_lifecycle', NEW.id,
        bokheim_mutation('directory_lifecycle', NEW.id, CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_dir_name_update
AFTER UPDATE OF name ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000'
  AND OLD.name IS NOT NEW.name
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_name', NEW.id,
        bokheim_mutation('directory_name', NEW.id, NEW.name),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_dir_parent_update
AFTER UPDATE OF parent_id ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000' AND OLD.parent_id IS NOT NEW.parent_id
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES ('directory_parent', NEW.id, bokheim_mutation('directory_parent', NEW.id, NEW.parent_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0)
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,
        mutation_id=excluded.mutation_id, body=excluded.body, changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_dir_lifecycle_update
AFTER UPDATE OF deleted_at, purged_at ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000'
  AND (OLD.deleted_at IS NOT NEW.deleted_at OR OLD.purged_at IS NOT NEW.purged_at)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES ('directory_lifecycle', NEW.id, bokheim_mutation('directory_lifecycle', NEW.id, CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END)
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,
        mutation_id=excluded.mutation_id, body=excluded.body, changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_added
AFTER INSERT ON book
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), contributors_for_book(NEW.row_id), NEW.book_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_added_reactivate
AFTER UPDATE OF deleted_at, purged_at ON book
WHEN NEW.deleted_at IS NULL AND (OLD.deleted_at IS NOT NULL OR OLD.purged_at IS NOT NULL)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), contributors_for_book(NEW.row_id), NEW.book_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_deleted
AFTER UPDATE OF deleted_at, purged_at ON book
WHEN NEW.deleted_at IS NOT NULL AND NEW.purged_at IS NULL
  AND (OLD.deleted_at IS NULL OR OLD.purged_at IS NOT NULL)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 1, '', X'', X'', 0, '', NEW.trash_origin_dir_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 1
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_purged
AFTER UPDATE OF purged_at ON book
WHEN NEW.purged_at IS NOT NULL AND OLD.purged_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 2, '', X'', X'', 0, '', NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 2
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;


-- Folder ownership is derived state, not synchronized state. These triggers
-- deliberately run for both local and remote placement writes.
CREATE TRIGGER trig_folder_book_ownership_insert
AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
BEGIN
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestors(id) AS (
        SELECT id FROM dir WHERE id=NEW.dir_id AND deleted_at IS NULL
        UNION ALL
        SELECT parent.id
        FROM ancestors AS child
        JOIN dir AS current ON current.id=child.id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT id, NEW.book_row_id, 1, CASE WHEN NEW.is_downloaded=1 THEN 1 ELSE 0 END
    FROM ancestors
    WHERE true
    ON CONFLICT(folder_id, book_row_id) DO UPDATE SET
        placement_count=folder_book_ownership.placement_count + 1,
        downloaded_placement_count=folder_book_ownership.downloaded_placement_count + excluded.downloaded_placement_count;
END;

-- Remove OLD's contribution before any tracked placement attribute changes.
-- Rows at one are deleted first so the positive-count constraint is never
-- transiently violated.
CREATE TRIGGER trig_folder_book_ownership_update_remove
BEFORE UPDATE OF dir_id, book_row_id, deleted_at, is_downloaded ON book_dir
WHEN OLD.deleted_at IS NULL
BEGIN
    DELETE FROM folder_book_ownership
    WHERE book_row_id=OLD.book_row_id
      AND placement_count=1
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
    UPDATE folder_book_ownership
    SET placement_count=placement_count - 1,
        downloaded_placement_count=downloaded_placement_count - CASE WHEN OLD.is_downloaded=1 THEN 1 ELSE 0 END
    WHERE book_row_id=OLD.book_row_id
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
END;

CREATE TRIGGER trig_folder_book_ownership_update_add
AFTER UPDATE OF dir_id, book_row_id, deleted_at, is_downloaded ON book_dir
WHEN NEW.deleted_at IS NULL
BEGIN
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestors(id) AS (
        SELECT id FROM dir WHERE id=NEW.dir_id AND deleted_at IS NULL
        UNION ALL
        SELECT parent.id
        FROM ancestors AS child
        JOIN dir AS current ON current.id=child.id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT id, NEW.book_row_id, 1, CASE WHEN NEW.is_downloaded=1 THEN 1 ELSE 0 END
    FROM ancestors
    WHERE true
    ON CONFLICT(folder_id, book_row_id) DO UPDATE SET
        placement_count=folder_book_ownership.placement_count + 1,
        downloaded_placement_count=folder_book_ownership.downloaded_placement_count + excluded.downloaded_placement_count;
END;

CREATE TRIGGER trig_folder_book_ownership_delete
BEFORE DELETE ON book_dir
WHEN OLD.deleted_at IS NULL
BEGIN
    DELETE FROM folder_book_ownership
    WHERE book_row_id=OLD.book_row_id
      AND placement_count=1
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
    UPDATE folder_book_ownership
    SET placement_count=placement_count - 1,
        downloaded_placement_count=downloaded_placement_count - CASE WHEN OLD.is_downloaded=1 THEN 1 ELSE 0 END
    WHERE book_row_id=OLD.book_row_id
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
END;

-- Folder hierarchy changes are much rarer than placement writes. Rebuilding
-- once gives exact ownership across the moved subtree and both ancestor
-- chains. Remote projection suppresses this per-row rebuild and performs one
-- rebuild after its batch instead.
CREATE TRIGGER trig_folder_book_ownership_directory_update
AFTER UPDATE OF parent_id, deleted_at, purged_at ON dir
WHEN (OLD.parent_id IS NOT NEW.parent_id OR OLD.deleted_at IS NOT NEW.deleted_at OR OLD.purged_at IS NOT NEW.purged_at)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') != 'remote'
BEGIN
    DELETE FROM folder_book_ownership;
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestry(descendant_id, ancestor_id) AS (
        SELECT id, id FROM dir WHERE deleted_at IS NULL
        UNION ALL
        SELECT ancestry.descendant_id, parent.id
        FROM ancestry
        JOIN dir AS current ON current.id=ancestry.ancestor_id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT ancestry.ancestor_id, placement.book_row_id, COUNT(*),
           SUM(CASE WHEN placement.is_downloaded=1 THEN 1 ELSE 0 END)
    FROM book_dir AS placement
    JOIN ancestry ON ancestry.descendant_id=placement.dir_id
    WHERE placement.deleted_at IS NULL
    GROUP BY ancestry.ancestor_id, placement.book_row_id;
END;

CREATE TRIGGER trig_folder_book_ownership_directory_delete
AFTER DELETE ON dir
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') != 'remote'
BEGIN
    DELETE FROM folder_book_ownership;
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestry(descendant_id, ancestor_id) AS (
        SELECT id, id FROM dir WHERE deleted_at IS NULL
        UNION ALL
        SELECT ancestry.descendant_id, parent.id
        FROM ancestry
        JOIN dir AS current ON current.id=ancestry.ancestor_id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT ancestry.ancestor_id, placement.book_row_id, COUNT(*),
           SUM(CASE WHEN placement.is_downloaded=1 THEN 1 ELSE 0 END)
    FROM book_dir AS placement
    JOIN ancestry ON ancestry.descendant_id=placement.dir_id
    WHERE placement.deleted_at IS NULL
    GROUP BY ancestry.ancestor_id, placement.book_row_id;
END;

CREATE TRIGGER trig_book_dir_added_insert
AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'placement', (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.dir_id,
        bokheim_mutation('placement', NEW.dir_id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), 1, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    ) ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_dir_added_reactivate
AFTER UPDATE OF deleted_at, dir_id, book_row_id ON book_dir
WHEN NEW.deleted_at IS NULL
  AND (OLD.deleted_at IS NOT NULL OR OLD.dir_id IS NOT NEW.dir_id OR OLD.book_row_id IS NOT NEW.book_row_id)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'placement', (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.dir_id,
        bokheim_mutation('placement', NEW.dir_id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), 1, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    ) ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_dir_deleted
AFTER UPDATE OF deleted_at ON book_dir
WHEN NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'placement', (SELECT content_hash FROM book WHERE row_id=OLD.book_row_id), OLD.dir_id,
        bokheim_mutation('placement', OLD.dir_id, (SELECT content_hash FROM book WHERE row_id=OLD.book_row_id), 0, NEW.trash_origin_dir_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 1
    ) ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_read_pos
AFTER UPDATE OF read_pos, read_progress ON book
WHEN NEW.read_pos IS NOT NULL
  AND (COALESCE(OLD.read_pos, '') != COALESCE(NEW.read_pos, '') OR COALESCE(OLD.read_progress, 0) != COALESCE(NEW.read_progress, 0))
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'reading_position', NEW.content_hash,
        bokheim_mutation('reading_position', NEW.content_hash, NEW.read_pos, COALESCE(NEW.read_progress, 0)),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_annotation_state_insert
AFTER INSERT ON annotation
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.anchor_kind, NEW.anchor_cfi, NEW.anchor_page, NEW.anchor_rects, NEW.anchor_fallback_cfi,
            NEW.exact_text, NEW.style, NEW.color, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_annotation_state_update
AFTER UPDATE OF anchor_kind, anchor_cfi, anchor_page, anchor_rects, anchor_fallback_cfi, exact_text, style, color, note, modified_at, deleted_at ON annotation
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.anchor_kind, NEW.anchor_cfi, NEW.anchor_page, NEW.anchor_rects, NEW.anchor_fallback_cfi,
            NEW.exact_text, NEW.style, NEW.color, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_sync_outbox_stamp_insert
AFTER INSERT ON sync_outbox
BEGIN
    INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
    VALUES (NEW.state_kind,
        NEW.state_key, NEW.state_subkey, NEW.changed_at, NEW.conflict_rank,
        COALESCE((SELECT replica_id FROM sync_metadata WHERE singleton = 1), ''),NEW.replica_seq,NEW.mutation_id)
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,
        replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;
END;

CREATE TRIGGER trig_sync_outbox_stamp_update
AFTER UPDATE OF replica_seq ON sync_outbox
BEGIN
    INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
    VALUES (NEW.state_kind, NEW.state_key, NEW.state_subkey, NEW.changed_at, NEW.conflict_rank,
        COALESCE((SELECT replica_id FROM sync_metadata WHERE singleton = 1), ''),NEW.replica_seq,NEW.mutation_id)
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,
        replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;
END;
-- Navigation travels because a replica cannot always rebuild it: an audiobook's
-- chapters may have come from enrichment rather than from the file, and a
-- replica that has not downloaded a book has no file to rebuild from at all.
CREATE TRIGGER IF NOT EXISTS book_toc_insert AFTER INSERT ON book_toc
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'book_toc',NEW.content_hash,'',bokheim_mutation('book_toc',NEW.content_hash,NEW.toc_json),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER IF NOT EXISTS book_toc_update AFTER UPDATE ON book_toc
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'book_toc',NEW.content_hash,'',bokheim_mutation('book_toc',NEW.content_hash,NEW.toc_json),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER IF NOT EXISTS book_toc_purge AFTER UPDATE OF purged_at ON book
WHEN NEW.purged_at IS NOT NULL
BEGIN
    DELETE FROM book_toc WHERE content_hash=NEW.content_hash;
    DELETE FROM sync_outbox WHERE state_kind='book_toc' AND state_key=NEW.content_hash;
END;

-- name: schema_views &
-- Browse views.
CREATE VIEW IF NOT EXISTS dir_paths AS
WITH RECURSIVE dir_path(id, path) AS (
    -- root produces empty path
    SELECT id, ''
    FROM dir
    WHERE parent_id = id
      AND deleted_at IS NULL

    UNION ALL

    -- children append normally
    SELECT
        d.id,
        CASE
            WHEN dp.path = ''
                THEN d.name
            ELSE dp.path || '/' || d.name
        END
    FROM dir d
    JOIN dir_path dp ON d.parent_id = dp.id
    WHERE d.deleted_at IS NULL
      AND d.id <> d.parent_id
)
SELECT id, path FROM dir_path;

-- Author-role subset of the merged contributor register. Kept as a view so
-- the `role = X'617574'` (MarcRelatorCode::AUTHOR) literal is not repeated
-- across every query that only cares about credited authors.
CREATE VIEW IF NOT EXISTS author_credit AS
SELECT book_row_id, position, author_identity_id, name AS credited_name, sort_name
FROM book_contributor
WHERE role = X'617574';

CREATE VIEW IF NOT EXISTS book_cards AS
SELECT
    book.row_id AS book_row_id,
    book.content_hash,
    book.title,
    COALESCE(book.added_at, 0) AS added_at,
    (SELECT value FROM book_value
     WHERE book_row_id=book.row_id AND kind='alternate_title' AND lower(qualifier)='subtitle'
     ORDER BY position LIMIT 1) AS subtitle,
    COALESCE((
        SELECT group_concat(credit.credited_name, ', ')
        FROM (SELECT credited_name FROM author_credit WHERE book_row_id = book.row_id ORDER BY position) AS credit
    ), '') AS author,
    COALESCE((SELECT description FROM book_description WHERE book_row_id=book.row_id), '') AS description,
    CASE
        WHEN book.read_progress > 0 THEN book.read_progress
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos NOT IN ('', '0')
            AND CAST(book.read_pos AS REAL) > 0
            AND CAST(book.read_pos AS REAL) <= 1
            THEN CAST(book.read_pos AS REAL) * 100
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos NOT IN ('', '0')
            AND CAST(book.read_pos AS REAL) > 1
            AND CAST(book.read_pos AS REAL) <= 100
            THEN CAST(book.read_pos AS REAL)
        ELSE 0
    END AS progress,
    EXISTS (
        SELECT 1 FROM book_dir placement
        WHERE placement.book_row_id = book.row_id
          AND placement.deleted_at IS NULL
          AND placement.is_downloaded = 1
    ) AS downloaded,
    EXISTS (
        SELECT 1 FROM book_dir placement
        JOIN download_requests request
          ON request.target_kind = 'book'
         AND request.target_key = book.content_hash
        WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
    ) AS download_requested,
    CASE lower(book.format)
        WHEN 'pdf' THEN 2
        WHEN 'm4b' THEN 4
        ELSE 1
    END AS format_category,
    (
        SELECT metadata.duration_ms FROM book_dir placement
        JOIN audiobook_metadata metadata ON metadata.content_hash = book.content_hash
        WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
        ORDER BY placement.rowid DESC LIMIT 1
    ) AS audiobook_duration_ms,
    (
        SELECT toc.entry_count FROM book_dir placement
        JOIN book_toc toc ON toc.content_hash = book.content_hash
        JOIN audiobook_metadata metadata ON metadata.content_hash = book.content_hash
        WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
        ORDER BY placement.rowid DESC LIMIT 1
    ) AS audiobook_chapter_count
FROM book
WHERE book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND EXISTS (
      SELECT 1 FROM book_dir placement
      WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
  );
