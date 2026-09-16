INSERT INTO subject_book_ownership(subject_path,book_row_id,assignment_count)
WITH RECURSIVE routes(origin,path,parent_path) AS (
    SELECT r.path,r.path,r.parent_path FROM book_unified_concept a
    JOIN curated.unified_concept_route r ON r.concept_id=a.concept_id WHERE a.book_row_id=?1
    UNION ALL
    SELECT child.origin,r.path,r.parent_path FROM routes child
    JOIN curated.unified_concept_route r ON r.path=child.parent_path WHERE child.path!=child.parent_path
) SELECT path,?1,COUNT(*) FROM routes GROUP BY path;
