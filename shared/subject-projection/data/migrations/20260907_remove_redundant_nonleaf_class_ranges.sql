-- A same-endpoint class range duplicates an identical broad exact selector on
-- these navigational nodes.  Keep the selector and remove the redundant range.

DELETE FROM lcc_range AS ranges
WHERE ranges.start_code=ranges.end_code
  AND ranges.concept_id IN (11215,5083,4494,5079,4487,5369,5381,4628)
  AND EXISTS (
    SELECT 1 FROM lcc_selector AS selectors
    WHERE selectors.concept_id=ranges.concept_id
      AND selectors.selector=ranges.start_code
  );
