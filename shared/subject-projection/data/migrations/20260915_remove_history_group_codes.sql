-- Grouping nodes should not own direct current codes; their children carry
-- the regional, national, and leaf-level classifications.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM lcc_selector
WHERE concept_id IN (3319,3379,3395,4249);

COMMIT;
