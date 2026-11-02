-- Apply approved candidates 41-69 and restore coded ancestry for Pathology/Painting.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE approved_parent_move(concept_id INTEGER PRIMARY KEY,old_parent_id INTEGER NOT NULL,new_parent_id INTEGER NOT NULL);
INSERT INTO approved_parent_move VALUES
(37,5572,36),
(65,5572,36),
(72,5577,36),
(141,5574,36),
(88,5574,36),
(98,55099,36),
(35,55109,2),
(19,55110,2),
(25,55110,2),
(17,55109,2),
(862,55107,856),
(896,55108,856),
(892,55108,856),
(614,55072,601),
(5700,55121,193),
(5785,55194,2854),
(5229,55056,5142),
(5183,55057,5142),
(5230,55057,5142),
(4961,55115,4916),
(5651,55064,5384),
(11206,55064,5384),
(1664,5641,1387),
(1588,5642,1387),
(1717,5641,1387),
(2032,55209,1950),
(1992,5680,1950),
(3768,55002,3702),
(4664,55049,4628),
(2801,2535,55202),
(1032,969,5669);
CREATE TEMP TABLE approved_move_guard(valid INTEGER NOT NULL CHECK(valid=1));
INSERT INTO approved_move_guard SELECT count(*)=31 FROM approved_parent_move m JOIN concept_parent p ON p.concept_id=m.concept_id AND p.parent_concept_id=m.old_parent_id WHERE NOT EXISTS(SELECT 1 FROM concept_parent e WHERE e.concept_id=m.concept_id AND e.parent_concept_id=m.new_parent_id);
UPDATE concept_parent AS p SET parent_concept_id=(SELECT new_parent_id FROM approved_parent_move m WHERE m.concept_id=p.concept_id AND m.old_parent_id=p.parent_concept_id) WHERE EXISTS(SELECT 1 FROM approved_parent_move m WHERE m.concept_id=p.concept_id AND m.old_parent_id=p.parent_concept_id);
INSERT INTO approved_move_guard SELECT changes()=31;
DROP TABLE approved_move_guard;
DROP TABLE approved_parent_move;
COMMIT;
