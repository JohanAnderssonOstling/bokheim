-- Exact LCC class headings at their existing broad discipline subjects.
-- These are exact bare codes, not wildcard selectors for unknown notations.
BEGIN IMMEDIATE;
-- History of the Americas
INSERT INTO lcc_selector(concept_id,selector) VALUES(3379,'F');
-- Medicine
INSERT INTO lcc_selector(concept_id,selector) VALUES(2725,'R');
-- Science & Nature
INSERT INTO lcc_selector(concept_id,selector) VALUES(4779,'Q');
-- Social Sciences
INSERT INTO lcc_selector(concept_id,selector) VALUES(5055,'H');
-- Education
INSERT INTO lcc_selector(concept_id,selector) VALUES(2191,'L');
-- Technology & Engineering
INSERT INTO lcc_selector(concept_id,selector) VALUES(5563,'T');
-- Agriculture
INSERT INTO lcc_selector(concept_id,selector) VALUES(2,'S');
-- Politics & Government
INSERT INTO lcc_selector(concept_id,selector) VALUES(5326,'J');
-- Public Finance
INSERT INTO lcc_selector(concept_id,selector) VALUES(5209,'HJ');
-- German History
INSERT INTO lcc_selector(concept_id,selector) VALUES(3524,'DD');
COMMIT;
