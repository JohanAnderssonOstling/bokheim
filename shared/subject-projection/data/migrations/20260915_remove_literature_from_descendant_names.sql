-- The Literature root already supplies the context. Remove repeated
-- Literature/Literatures wording from all descendants and clean labels that
-- would otherwise begin with punctuation or become empty.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE literature_descendants (concept_id INTEGER PRIMARY KEY);
WITH RECURSIVE tree(id) AS (
  SELECT 3846
  UNION ALL
  SELECT cp.concept_id FROM concept_parent cp JOIN tree ON cp.parent_concept_id=tree.id
)
INSERT OR IGNORE INTO literature_descendants
SELECT id FROM tree WHERE id <> 3846;

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' & Literatures',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' & Literature',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' & literatures',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' & literature',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' in Literature',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' in literature',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' Literatures',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' Literature',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' literatures',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,' literature',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,'Literature & ',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label=trim(replace(preferred_label,'Literature, ',''))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants);

UPDATE concept
SET preferred_label='General'
WHERE concept_id IN (SELECT concept_id FROM literature_descendants)
  AND preferred_label IN ('','Literature','literature');

UPDATE concept
SET preferred_label=trim(substr(preferred_label,3))
WHERE concept_id IN (SELECT concept_id FROM literature_descendants)
  AND (preferred_label LIKE ', %' OR preferred_label LIKE '& %');

UPDATE concept
SET preferred_label='General'
WHERE concept_id IN (SELECT concept_id FROM literature_descendants)
  AND preferred_label='';

DROP TABLE literature_descendants;
COMMIT;
