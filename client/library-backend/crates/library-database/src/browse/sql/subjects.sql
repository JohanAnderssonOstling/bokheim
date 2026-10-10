-- name: subject_assignments?
-- Only books with a live placement participate in the subject hierarchy.
SELECT assignment.book_row_id,route.route_id
FROM book_unified_concept assignment
JOIN book ON book.row_id=assignment.book_row_id
JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id);


-- name: concept_routes?
-- param: concept_ids: &str
-- Every route each concept appears at, for books outside the library.
SELECT route.concept_id,route.route_id
FROM json_each(:concept_ids) selected
JOIN curated.unified_concept_route route ON route.concept_id=selected.value;

-- name: taxonomy_route_parent ->
-- param: route_id: &SqlParam<'_>
SELECT route.parent_route_id,concept.preferred_label
FROM curated.unified_concept_route route JOIN curated.concept concept USING(concept_id)
WHERE route.route_id=:route_id;
