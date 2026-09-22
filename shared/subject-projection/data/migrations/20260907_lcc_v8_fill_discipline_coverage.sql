-- Reference-supported discipline ranges. Existing finer selectors retain precedence.
BEGIN IMMEDIATE;
-- Christianity
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BR1','BR1725',4582);
-- Christian Denominations
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BX1','BX9999',4558);
-- Religion
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BL1','BL2790',4549);
-- Anthropology
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('GN1','GN890',5056);
-- Germanic Languages
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('PD1','PD7159',3749);
-- Judaism
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BM1','BM990',4615);
-- Biography
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('CT21','CT9999',3216);
-- Buddhism
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BQ1','BQ9800',4553);
-- History of education
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('LA1','LA2396',2203);
-- Economic history
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('HC1','HC1085.2',5091);
-- Bible
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BS1','BS2970',4551);
-- Philosophy
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BD10','BD701',4487);
-- Metaphysics & Ontology
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('BD95','BD131',4544);
-- Criminology
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('HV6001','HV7220.5',5463);
-- Elements in the population
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('E184','E185.98',10756);
COMMIT;
