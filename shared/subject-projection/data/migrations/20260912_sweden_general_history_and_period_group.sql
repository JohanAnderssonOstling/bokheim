-- General Swedish histories belong to the substantive country subject.
-- Chronological subjects sit under an uncoded navigation heading.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM lcc_range WHERE concept_id=13935 AND start_code='DL643' AND end_code='DL659.Z';
INSERT INTO lcc_range VALUES('DL643','DL651',3584);
INSERT INTO lcc_range VALUES('DL654','DL659.Z',13935);
INSERT INTO concept(preferred_label) VALUES('By Period');
CREATE TEMP TABLE sweden_period_group AS SELECT last_insert_rowid() AS id;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id,3584,0 FROM sweden_period_group;
UPDATE concept_parent SET parent_concept_id=(SELECT id FROM sweden_period_group)
WHERE parent_concept_id=3584 AND concept_id IN
(13936,11555,10665,10644,10206,10132,11556,10138,11557);
DROP TABLE sweden_period_group;
COMMIT;
