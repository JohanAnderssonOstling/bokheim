-- Canadian history: LAC's FC alternate schedule and its documented F5000 predecessor.
-- FC current outline includes Nunavut at FC4301-4350.
BEGIN IMMEDIATE;
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('FC1','FC4350',3400);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('F5000','F5999',3400);
INSERT INTO lcc_selector(concept_id,selector) VALUES(3400,'FC');
COMMIT;
