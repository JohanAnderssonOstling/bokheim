-- Keep only the three official European subdivisions. BL660-BL2685.2 is the
-- enclosing religion schedule, not an Early European Religions selector.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:early-european-religions:classical','Classical Religion & Mythology'),
 ('bokheim:early-european-religions:germanic-norse','Germanic & Norse Mythology'),
 ('bokheim:early-european-religions:other','Other Early European Religions');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:early-european-religions:classical','bokheim:religion:european',0,100),
 ('bokheim:early-european-religions:germanic-norse','bokheim:religion:european',0,200),
 ('bokheim:early-european-religions:other','bokheim:religion:european',0,300);
DELETE FROM source_selector WHERE concept_id='bokheim:religion:european';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:early-european-religions:classical','lcc','BL700..BL820@5',0),
 ('bokheim:early-european-religions:germanic-norse','lcc','BL830..BL875@5',0),
 ('bokheim:early-european-religions:other','lcc','BL900..BL975.2@5',0);
COMMIT;
