BEGIN IMMEDIATE;

-- Sociology is a deliberately broad leaf. Keep its BISAC facets, but replace
-- the generated, overlapping HM subranges and singleton selectors with the
-- official class span. A permissive range is appropriate because this node
-- has no children and therefore owns the pruned detail below HM.
DELETE FROM source_selector
WHERE concept_id = 'unified:38ac9c54-af17-54e5-aa17-0613f079c100'
  AND system_id = 'lcc';

INSERT INTO source_selector (concept_id, system_id, selector, ordinal)
VALUES
  ('unified:38ac9c54-af17-54e5-aa17-0613f079c100', 'lcc', 'HM', 0),
  ('unified:38ac9c54-af17-54e5-aa17-0613f079c100', 'lcc', 'HM1..HM1281@1', 1);

COMMIT;
