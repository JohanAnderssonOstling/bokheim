-- Existing specialist leaves cover QA821-QA939. Exact topics override the
-- opening reference/theory leaf.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53359,'Analytic Mechanics Reference & Theory');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53359,4923,0);

DELETE FROM lcc_range WHERE concept_id=4923;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QA801','QA820.Z',53359);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (4923,'QA801-939');
