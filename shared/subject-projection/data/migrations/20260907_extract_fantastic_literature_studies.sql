-- Extract fantastic-literature studies from general literary themes.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES(52323,'Fantastic Literature Studies');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52323,8293,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES(52323,'PN56.F34');
COMMIT;
