SELECT r.path,r.label,c.book_count,
    (SELECT COUNT(*) FROM subject_browse_member m WHERE m.path=r.path
        AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=m.book_row_id AND p.deleted_at IS NULL AND p.is_downloaded=1))
FROM subject_browse_child edge JOIN library_subject r ON r.path=edge.path
JOIN subject_browse_count c ON c.path=r.path WHERE edge.location=?1
ORDER BY CASE WHEN ?2=1 THEN c.book_count END DESC,normalize_search_text(r.label),r.label,r.path
