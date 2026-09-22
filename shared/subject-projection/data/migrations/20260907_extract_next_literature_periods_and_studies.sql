-- Extract high-use literature topics, genres and national traditions.
-- Narrow ranges override the retained general/residual parent coverage.
PRAGMA foreign_keys=ON;
BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES(52010,'Polish Literature, 1961–2000');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52010,12525,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PG','7159','7185',52010);

INSERT INTO concept(concept_id,preferred_label) VALUES(52011,'Polish Literary Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52011,12525,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PG','7012','7130',52011);

INSERT INTO concept(concept_id,preferred_label) VALUES(52012,'Swedish Literature, 1961–2000');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52012,4473,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PT','9876','9876.36',52012);

INSERT INTO concept(concept_id,preferred_label) VALUES(52013,'Swedish Literature, 1900–1960');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52013,4473,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PT','9870','9875',52013);

INSERT INTO concept(concept_id,preferred_label) VALUES(52014,'Dutch Literature, 1961–2000');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52014,3899,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PT','5881','5881.36',52014);

INSERT INTO concept(concept_id,preferred_label) VALUES(52015,'Dutch Literature, 1800–1960');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52015,3899,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PT','5800','5880',52015);

INSERT INTO concept(concept_id,preferred_label) VALUES(52016,'Japanese Poetry Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52016,12532,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','727','733',52016);

INSERT INTO concept(concept_id,preferred_label) VALUES(52017,'French Poetry Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52017,7795,0);

UPDATE lcc_range SET concept_id=52017 WHERE class_letters='PQ' AND start_value='400' AND end_value='491' AND concept_id=4239;

INSERT INTO concept(concept_id,preferred_label) VALUES(52018,'French Drama Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52018,7795,0);

UPDATE lcc_range SET concept_id=52018 WHERE class_letters='PQ' AND start_value='500' AND end_value='591' AND concept_id=4239;

INSERT INTO concept(concept_id,preferred_label) VALUES(52019,'French Prose Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52019,7795,0);

UPDATE lcc_range SET concept_id=52019 WHERE class_letters='PQ' AND start_value='601' AND end_value='771' AND concept_id=4239;

INSERT INTO concept(concept_id,preferred_label) VALUES(52020,'Korean Poetry Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52020,12634,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','959','961.8',52020);

INSERT INTO concept(concept_id,preferred_label) VALUES(52021,'Korean Prose Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52021,12634,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','965','967.8',52021);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(4149,11910,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PR','9387','9387.9',4149);

COMMIT;
