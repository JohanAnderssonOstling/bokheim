-- The existing History, Regional, and Special Schools leaves cover T66-T173.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53366,'Technical Education Reference & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53366,451,0);

DELETE FROM lcc_range
WHERE concept_id=451
  AND start_code='T61'
  AND end_code='T173';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('T61','T65.5.Z',53366);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (451,'T61-173');
