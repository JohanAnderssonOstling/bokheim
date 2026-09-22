-- Labels must differ from their direct parents after case normalization.
-- Keep the subject identities, hierarchy and classification assignments.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE label_fixes(concept_id INTEGER PRIMARY KEY,label TEXT NOT NULL);
INSERT INTO label_fixes VALUES
 (3868,'General'),
 (13577,'Literature'),
 (12947,'History & Profession'),
 (6860,'General'),
 (8705,'General'),
 (8590,'General');
INSERT INTO source_label(concept_id,label,ordinal)
SELECT c.concept_id,c.preferred_label,
 coalesce((SELECT max(s.ordinal)+1 FROM source_label s WHERE s.concept_id=c.concept_id),0)
FROM concept c JOIN label_fixes f USING(concept_id)
WHERE c.preferred_label<>f.label AND NOT EXISTS
 (SELECT 1 FROM source_label s WHERE s.concept_id=c.concept_id AND s.label=c.preferred_label);
UPDATE concept SET preferred_label=f.label FROM label_fixes f
WHERE concept.concept_id=f.concept_id;
DROP TABLE label_fixes;
COMMIT;
