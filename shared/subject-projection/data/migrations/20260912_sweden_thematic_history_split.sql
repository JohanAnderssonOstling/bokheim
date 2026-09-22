-- Split Swedish thematic history using the detailed LCC schedule:
-- DL654 military; DL656 naval; DL658–659 political/diplomatic and foreign relations.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label='Political History & Foreign Relations' WHERE concept_id=13935;
DELETE FROM lcc_range WHERE concept_id=13935 AND start_code='DL654' AND end_code='DL659.Z';
INSERT INTO lcc_range VALUES('DL658','DL659',13935);
INSERT INTO concept(preferred_label) VALUES('Military & Naval History');
CREATE TEMP TABLE sweden_military_node AS SELECT last_insert_rowid() AS id;
INSERT INTO concept_parent SELECT id,3584,0 FROM sweden_military_node;
INSERT INTO lcc_selector SELECT id,'DL654' FROM sweden_military_node;
INSERT INTO lcc_selector SELECT id,'DL656' FROM sweden_military_node;
DROP TABLE sweden_military_node;
COMMIT;
