-- High-impact literature, film, and mathematics leaf-range consolidation.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Modern English literary history, including its separately scheduled 21st century.
DELETE FROM source_selector WHERE concept_id=3925 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (3925,'lcc','PR401..PR479'),
  (3925,'lcc','PR481..PR488');

-- Mathematics study and teaching: official QA10.92-QA20 span.
DELETE FROM source_selector WHERE concept_id=4969 AND system_id='lcc';
INSERT INTO source_selector VALUES(4969,'lcc','QA10.92..QA20');

-- National and regional motion-picture histories occupy the country Cutter block.
DELETE FROM source_selector WHERE concept_id=12963 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (12963,'lcc','PN1993.5.A..PN1993.5.Z'),
  (12963,'lcc','PN1993.6');

COMMIT;
