-- Apply the 40 approved subject moves; keep concept identities, ordinals and selectors.
-- The guard rejects a different starting graph. Apply once to the reviewed source.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE reviewed_parent_move(concept_id INTEGER PRIMARY KEY,old_parent_id INTEGER NOT NULL,new_parent_id INTEGER NOT NULL);
INSERT INTO reviewed_parent_move VALUES
    (62,55098,36),
    (152,55098,36),
    (168,55098,36),
    (163,55098,36),
    (5575,55098,36),
    (5698,55122,193),
    (2854,54981,2535),
    (2747,54978,2535),
    (2707,55089,54982),
    (2801,55202,2535),
    (2563,54982,2535),
    (5030,55015,4779),
    (4803,55015,4779),
    (4780,55015,4779),
    (4789,55014,4779),
    (4507,54959,4487),
    (2203,5675,2169),
    (2315,5673,54996),
    (5596,55044,5086),
    (5598,55045,5086),
    (873,55108,856),
    (907,55107,856),
    (1032,5669,969),
    (4848,55144,4789),
    (4790,55144,4789),
    (4863,55144,4789),
    (5717,55114,4916),
    (5719,55114,4916),
    (4939,55114,4916),
    (2871,55194,2854),
    (2579,55090,54982),
    (2780,55088,54982),
    (3063,55061,3042),
    (5661,54956,5488),
    (5663,54956,5488),
    (1529,5639,1387),
    (1431,5638,1387),
    (1595,5638,1387),
    (4751,55007,4671),
    (4752,55007,4671);
CREATE TEMP TABLE reviewed_move_guard(valid INTEGER NOT NULL CHECK(valid=1));
INSERT INTO reviewed_move_guard SELECT count(*)=40 FROM reviewed_parent_move m JOIN concept_parent p ON p.concept_id=m.concept_id AND p.parent_concept_id=m.old_parent_id WHERE NOT EXISTS(SELECT 1 FROM concept_parent e WHERE e.concept_id=m.concept_id AND e.parent_concept_id=m.new_parent_id);
UPDATE concept_parent AS p SET parent_concept_id=(SELECT m.new_parent_id FROM reviewed_parent_move m WHERE m.concept_id=p.concept_id AND m.old_parent_id=p.parent_concept_id) WHERE EXISTS(SELECT 1 FROM reviewed_parent_move m WHERE m.concept_id=p.concept_id AND m.old_parent_id=p.parent_concept_id);
INSERT INTO reviewed_move_guard SELECT changes()=40;
DROP TABLE reviewed_move_guard;
DROP TABLE reviewed_parent_move;
COMMIT;
