-- Bare QK and QL remain exact fallbacks; their detailed fields already own
-- ranges below these navigational roots.

DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (4797,'QK1','QK989'),
  (5048,'QL1','QL991')
);
