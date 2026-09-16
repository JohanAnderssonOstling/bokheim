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

-- Count visible placements through non-hidden descendants. A folder containing
-- only empty or hidden descendants is empty for browsing too.
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
SELECT
    COUNT(*),
    COALESCE(SUM(CASE WHEN ownership.downloaded_placement_count > 0 THEN 1 ELSE 0 END), 0)
FROM folder_book_ownership AS ownership
JOIN book ON book.row_id = ownership.book_row_id
WHERE ownership.folder_id = :dir_id
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
