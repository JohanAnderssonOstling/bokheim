PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _newspaper_region(label TEXT PRIMARY KEY,selector TEXT UNIQUE);
INSERT INTO _newspaper_region VALUES
 ('North America','071'),('British Isles','072'),('Central Europe','073'),
 ('France & Monaco','074'),('Italy','075'),('Iberia','076'),
 ('Eastern Europe & Russia','077'),('Other Regions','079');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),0
FROM _newspaper_region r JOIN concept c ON c.preferred_label=r.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent cp WHERE cp.concept_id=c.concept_id)
  AND c.concept_id=(SELECT min(c2.concept_id) FROM concept c2 WHERE c2.preferred_label=r.label AND NOT EXISTS(SELECT 1 FROM concept_parent cp2 WHERE cp2.concept_id=c2.concept_id));

INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT r.selector,c.concept_id FROM _newspaper_region r
JOIN concept_parent cp ON cp.parent_concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Newspapers')
JOIN concept c ON c.concept_id=cp.concept_id AND c.preferred_label=r.label;
DROP TABLE _newspaper_region;
COMMIT;
