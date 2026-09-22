PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _stat_region(label TEXT PRIMARY KEY,selector TEXT UNIQUE);
INSERT INTO _stat_region VALUES
 ('Europe','314'),('Asia','315'),('Africa','316'),('North America','317'),
 ('South America','318'),('Other Regions','319');
INSERT INTO concept(preferred_label)
SELECT r.label FROM _stat_region r WHERE NOT EXISTS(
 SELECT 1 FROM concept_parent cp JOIN concept c ON c.concept_id=cp.concept_id
 WHERE cp.parent_concept_id=12956 AND c.preferred_label=r.label
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,12956,0 FROM _stat_region r JOIN concept c ON c.concept_id=(
 SELECT max(c2.concept_id) FROM concept c2 WHERE c2.preferred_label=r.label
 AND NOT EXISTS(SELECT 1 FROM concept_parent cp WHERE cp.concept_id=c2.concept_id)
);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT r.selector,c.concept_id FROM _stat_region r
JOIN concept_parent cp ON cp.parent_concept_id=12956
JOIN concept c ON c.concept_id=cp.concept_id AND c.preferred_label=r.label;
DROP TABLE _stat_region;
COMMIT;
