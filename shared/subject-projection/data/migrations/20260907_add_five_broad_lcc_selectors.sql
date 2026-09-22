-- Five approved broad-class exact selectors; detailed selectors are unchanged.
BEGIN IMMEDIATE;
INSERT INTO lcc_selector(concept_id,selector) VALUES(969,'N');
INSERT INTO lcc_selector(concept_id,selector) VALUES(5276,'K');
INSERT INTO lcc_selector(concept_id,selector) VALUES(3319,'D');
INSERT INTO lcc_selector(concept_id,selector) VALUES(5381,'BF');
INSERT INTO lcc_selector(concept_id,selector) VALUES(720,'M');
UPDATE taxonomy_meta SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='release_id';
COMMIT;
