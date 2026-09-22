BEGIN;
-- Stable OpenLibrary code-use cache: seed technology ~148 matched uses,
-- propagation ~772, and SB125 training/pruning 49 direct uses. SB128 growth
-- regulators has only 5 direct uses and therefore stays on the parent.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code FROM lcc_range WHERE concept_id=5885;
DELETE FROM lcc_range WHERE concept_id=5885;
DELETE FROM lcc_selector WHERE concept_id=5885 AND selector='SB125';

CREATE TEMP TABLE _topic(label TEXT,selector TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _topic VALUES
 ('Seed Technology',NULL,'SB113.2','SB118.46'),
 ('Plant Propagation Methods',NULL,'SB119','SB124'),
 ('Training & Pruning','SB125',NULL,NULL);
INSERT INTO concept(preferred_label) SELECT label FROM _topic s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,5885,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM _topic s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=5885);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _topic s JOIN concept c ON c.preferred_label=s.label WHERE s.selector IS NOT NULL;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _topic s JOIN concept c ON c.preferred_label=s.label WHERE s.start_code IS NOT NULL;
DROP TABLE _topic;
COMMIT;
