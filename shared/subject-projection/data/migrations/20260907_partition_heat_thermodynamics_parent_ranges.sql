-- Thermometry and every later occupied division already have specialist leaves.
-- The opening QC251-QC269 material is reference, theory, and heat measurement.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53345,'Heat Reference, Theory & Measurement');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53345,5037,0);

DELETE FROM lcc_range WHERE concept_id=5037;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QC251','QC269.Z',53345);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5037,'QC251-338.5');
