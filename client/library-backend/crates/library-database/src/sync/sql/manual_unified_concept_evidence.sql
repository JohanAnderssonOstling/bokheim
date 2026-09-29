SELECT DISTINCT e.concept_id
FROM book_unified_concept_evidence e
JOIN curated.unified_concept_paths p ON p.concept_id=e.concept_id
WHERE e.book_row_id=?1 AND e.subject_position=?2
  AND e.source_system_id='unified' AND e.source_code=?3;
