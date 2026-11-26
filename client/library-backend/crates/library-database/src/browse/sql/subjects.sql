-- name: subject_assignments?
-- Only books with a live placement participate in the subject hierarchy.
SELECT assignment.book_row_id,route.route_id
FROM book_unified_concept assignment
JOIN book ON book.row_id=assignment.book_row_id
JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id)
ORDER BY assignment.book_row_id,route.route_id;

-- name: catalog_subject_assignments?
-- Resolve the catalog's concept assignments into the same book/route pairs.
SELECT assignment.book_row_id,route.route_id
FROM catalog_browse_concept assignment
JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id
ORDER BY assignment.book_row_id,route.route_id;

-- name: taxonomy_route_labels?
-- param: route_ids: &str
SELECT route.route_id,concept.preferred_label
FROM json_each(:route_ids) selected
JOIN curated.unified_concept_route route ON route.route_id=selected.value
JOIN curated.concept concept USING(concept_id);
