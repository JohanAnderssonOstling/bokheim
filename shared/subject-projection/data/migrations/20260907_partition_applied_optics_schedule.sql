-- Complete TA1501-TA1820 around existing holography, infrared, data-processing,
-- micro-optics, laser, electro-optics, acousto-optics, X-ray, and fiber leaves.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53382,'Applied Photonics Reference & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53382,277,0);

DELETE FROM lcc_range
WHERE concept_id=277
  AND start_code='TA1501'
  AND end_code='TA1820';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('TA1501','TA1539.9',53382);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (277,'TA1501-1820');

DELETE FROM lcc_range
WHERE concept_id=7238;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('TA1630','TA1653',7238);
