WITH RECURSIVE routes(path,parent_path,concept_id,label,direct) AS (
    SELECT r.path,r.parent_path,r.concept_id,r.label,1 FROM book_unified_concept a
    JOIN book b ON b.row_id=a.book_row_id JOIN curated.unified_concept_route r ON r.concept_id=a.concept_id
    WHERE b.row_id=?1 AND b.deleted_at IS NULL AND b.hidden_at IS NULL
      AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=b.row_id AND p.deleted_at IS NULL)
    UNION
    SELECT r.path,r.parent_path,r.concept_id,r.label,0 FROM routes child
    JOIN curated.unified_concept_route r ON r.path=child.parent_path WHERE child.path!=child.parent_path
) SELECT path,parent_path,concept_id,label,MAX(direct) FROM routes GROUP BY path
