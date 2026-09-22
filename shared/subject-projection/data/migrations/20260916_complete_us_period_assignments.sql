-- Verified against LOC's full 2025 E-F schedule, not only its outline.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Period fallbacks: more specific descendant subjects retain precedence.
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('E186','E199.999',3409),
 ('E201','E298.999',3424),
 ('E300','E453.999',3417),
 ('E456','E655.999',3408),
 ('E740','E889.999',3405),
 ('E891','E919.999',3406);
-- Expand the legacy abbreviated selector into a runtime-supported range.
DELETE FROM lcc_selector WHERE concept_id=12240 AND selector='E441-453';
INSERT INTO lcc_range VALUES('E441','E453.999',12240);
-- Remove fragmented fallbacks now contained by each century's full range.
DELETE FROM lcc_range WHERE concept_id=3405 AND start_code IN('E766','E806','E813','E835');
DELETE FROM lcc_range WHERE concept_id=3406 AND start_code IN('E902','E907','E912','E916');
DELETE FROM lcc_selector WHERE concept_id=3406 AND selector IN('E905','E906','E910','E911','E915','E919');
DELETE FROM lcc_range WHERE concept_id=3417 AND start_code='E302.5';

-- E840.4 is military/naval/air force history, NOT diplomacy.
UPDATE concept SET preferred_label='Military, 1961–2000' WHERE concept_id=51006;
UPDATE lcc_range SET end_code='E840.599' WHERE concept_id=51006 AND start_code='E840.4';
-- Route diplomacy through the existing twentieth-century diplomatic subject.
INSERT INTO lcc_range VALUES('E840','E840.299',51187);
-- Do not nest the whole-century diplomatic subject under the late century:
-- its narrower code range already takes precedence over the era fallback.
-- E351 and E351.2 etc. ARE War of 1812 in the full schedule; keep them.

-- Add century browsing for the early/middle nineteenth century, preserving
-- their valid cross-century umbrella path and existing stable IDs.
INSERT INTO concept_parent
 SELECT c.concept_id,3404,coalesce((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
 FROM concept c WHERE c.concept_id IN(3411,3414,3415,3408);

CREATE TEMP TABLE period_names(id INTEGER PRIMARY KEY,label TEXT NOT NULL);
INSERT INTO period_names VALUES
 (3412,'1865–1900'),(3413,'1961–2000'),
 (51086,'Biography, 1861–1865'),(51088,'Diplomacy'),
 (51089,'Military Operations, 1861–1865'),(51090,'Finance & Commerce'),
 (51092,'Armies & Troops, 1861–1865'),(51093,'Naval, 1861–1865'),
 (51094,'Personal Narratives'),(51095,'Prisoners & Prisons'),
 (51096,'Medicine & Hospitals'),(51097,'Relief & Charities'),
 (51098,'Memorials'),(51099,'Visual Material'),
 (51119,'Biography, 1775–1783'),(51120,'Politics, 1775–1783'),
 (51122,'Military Operations, 1775–1783'),(51123,'Diplomacy'),
 (51124,'Armies & Troops, 1775–1783'),(51125,'Naval, 1775–1783'),
 (51126,'Personal Narratives'),(51127,'Women'),(51129,'Espionage'),
 (51130,'Prisoners'),(51131,'Medicine'),(51132,'Memory & Culture'),
 (51144,'Biography, 1865–1900'),(51186,'Politics, 20th Century'),
 (51187,'Foreign Relations, 20th Century'),(51188,'Military, 20th Century'),
 (51189,'Naval, 20th Century'),(51190,'Biography, 20th Century'),
 (51337,'1607–1689'),(51338,'1689–1775');
-- Keep valid historical display names searchable. Date qualifiers remain
-- where multiple parents need them to distinguish sibling topics.
INSERT INTO source_label(concept_id,label,ordinal)
 SELECT c.concept_id,c.preferred_label,
 coalesce((SELECT max(ordinal)+1 FROM source_label s WHERE s.concept_id=c.concept_id),0)
 FROM concept c JOIN period_names n ON n.id=c.concept_id
 WHERE NOT EXISTS(SELECT 1 FROM source_label s WHERE s.concept_id=c.concept_id AND s.label=c.preferred_label);
UPDATE concept SET preferred_label=(SELECT label FROM period_names WHERE id=concept_id)
 WHERE concept_id IN(SELECT id FROM period_names);
DROP TABLE period_names;
COMMIT;
