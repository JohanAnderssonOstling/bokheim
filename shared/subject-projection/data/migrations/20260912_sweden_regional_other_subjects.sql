-- Keep Swedish geographic navigation groups uncoded.
-- Named places retain narrower assignments; Other children own fallback ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(preferred_label) VALUES('Other Provinces & Regions');
CREATE TEMP TABLE sweden_other_region AS SELECT last_insert_rowid() id;
INSERT INTO concept_parent SELECT id,13749,0 FROM sweden_other_region;
UPDATE lcc_range SET concept_id=(SELECT id FROM sweden_other_region) WHERE concept_id=13749;
INSERT INTO concept(preferred_label) VALUES('Other Cities & Towns');
CREATE TEMP TABLE sweden_other_city AS SELECT last_insert_rowid() id;
INSERT INTO concept_parent SELECT id,13750,0 FROM sweden_other_city;
UPDATE lcc_range SET concept_id=(SELECT id FROM sweden_other_city) WHERE concept_id=13750;
DROP TABLE sweden_other_region;
DROP TABLE sweden_other_city;
COMMIT;
