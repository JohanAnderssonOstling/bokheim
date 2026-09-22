BEGIN;

-- Resolve the two normalized endpoint collisions in favor of the specific
-- curated concepts, then discard their marker-bearing duplicates.
DELETE FROM lcc_range
WHERE (concept_id=10192 AND start_code='DG233' AND end_code='DG233.9@4')
   OR (concept_id=13585 AND start_code='PN1997' AND end_code='PN1997.85@3');

UPDATE lcc_range SET concept_id=10192
WHERE start_code='DG233' AND end_code='DG233.9';

UPDATE lcc_range SET concept_id=13585
WHERE start_code='PN1997' AND end_code='PN1997.85';

-- @3/@4/@5 are source hierarchy-depth markers, not parts of LCC notation.
UPDATE lcc_range
SET end_code=substr(end_code,1,instr(end_code,'@')-1)
WHERE instr(end_code,'@')>0;

-- These are unresolved schedule templates or edition/date annotations, not
-- exact classification notations and must never participate in lookup.
DELETE FROM lcc_selector
WHERE selector LIKE '%<date>%'
   OR selector GLOB '* [0-9][0-9][0-9][0-9]'
   OR selector GLOB '*[0-9][a-z]';

COMMIT;
