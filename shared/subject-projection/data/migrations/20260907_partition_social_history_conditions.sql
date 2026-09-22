-- Remove permissive overlaps from Social History & Conditions.  Exact broad
-- headings may remain on a nonleaf, but ranges must have one leaf owner.

DELETE FROM lcc_range
WHERE concept_id=5516
  AND start_code='HN'
  AND end_code='HN';

-- The broad Europe span overlaps the existing British, French and German
-- ranges.  Keep only observed high-use residual European schedule points as
-- exact matches rather than inventing unsupported gap boundaries.
DELETE FROM lcc_range
WHERE concept_id=52693
  AND start_code='HN371'
  AND end_code='HN650.8';

INSERT INTO lcc_selector(concept_id,selector)
VALUES
  (52693,'HN373'),
  (52693,'HN373.5'),
  (52693,'HN523.5');

-- These printed spans describe existing topical leaves, not the navigational
-- Social History & Conditions parent.
UPDATE lcc_selector SET concept_id=12417
WHERE concept_id=5516 AND selector IN('HN30','HN30-39','HN35-39');

UPDATE lcc_selector SET concept_id=52690
WHERE concept_id=5516 AND selector IN('HN41-46','HN43-46');

-- HN30-HN39 is wholly contained by the retained HN30-HN40 range.
DELETE FROM lcc_range
WHERE concept_id=12417
  AND start_code='HN30'
  AND end_code='HN39';
