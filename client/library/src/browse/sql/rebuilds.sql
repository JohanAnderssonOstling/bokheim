-- Rebuildable derived state supports hierarchy changes and recovery.
-- Normal placement writes are maintained incrementally by
-- triggers; hierarchy changes use this same rebuild statement.
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
