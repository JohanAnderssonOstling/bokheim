-- History supplies the subject context for its descendants. Preserve old
-- names as searchable source labels; keep identities and classification intact.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE history_names (
  concept_id INTEGER PRIMARY KEY,
  old_label TEXT NOT NULL,
  new_label TEXT NOT NULL
);
WITH RECURSIVE tree(id) AS (
  SELECT 3319
  UNION
  SELECT cp.concept_id FROM concept_parent cp JOIN tree ON cp.parent_concept_id=tree.id
)
INSERT INTO history_names
SELECT concept_id, preferred_label, trim(preferred_label)
FROM concept JOIN tree ON id=concept_id WHERE concept_id<>3319;

UPDATE history_names SET new_label=CASE
  WHEN new_label='History as a profession' THEN 'Profession'
  WHEN new_label='World histories' THEN 'General'
  WHEN new_label LIKE 'History of %' THEN substr(new_label,12)
  WHEN new_label LIKE 'History %' THEN substr(new_label,9)
  ELSE new_label END;
UPDATE history_names SET new_label=replace(replace(replace(
  new_label,' of History',''),' History',''),' history','');
-- Historical geography and historiography retain their distinct meanings.
UPDATE history_names SET new_label=replace(new_label,'Historical Eras','Eras');
UPDATE history_names SET new_label=upper(substr(new_label,1,1)) || substr(new_label,2);

-- Remove a repeated parent theme or place only when every immediate parent
-- supplies that same label. Multiple differing parents need their full context.
CREATE TEMP TABLE history_parent_names AS
SELECT cp.concept_id, min(p.new_label) AS parent_label
FROM concept_parent cp JOIN history_names p ON p.concept_id=cp.parent_concept_id
GROUP BY cp.concept_id
HAVING count(*)=(SELECT count(*) FROM concept_parent all_parents
                WHERE all_parents.concept_id=cp.concept_id)
   AND count(DISTINCT p.new_label)=1;
UPDATE history_names AS child SET new_label=CASE
  WHEN substr(child.new_label,1,length(p.parent_label)+1)=p.parent_label || ' '
    THEN substr(child.new_label,length(p.parent_label)+2)
  WHEN substr(child.new_label,-length(p.parent_label)-1)=' ' || p.parent_label
    THEN substr(child.new_label,1,length(child.new_label)-length(p.parent_label)-1)
  ELSE child.new_label END
FROM history_parent_names p
WHERE p.concept_id=child.concept_id AND length(p.parent_label)>=4
  AND p.parent_label NOT IN ('General','Other');
UPDATE history_names SET new_label=substr(new_label,3) WHERE new_label LIKE '& %';
UPDATE history_names SET new_label=upper(substr(new_label,1,1)) || substr(new_label,2);
-- DP302.M1 belongs to Regions & Administrative Divisions in the master
-- classification; DP350–DP374 is the separate Madrid local subject.
UPDATE history_names SET new_label='Madrid Region' WHERE concept_id=52060;

INSERT INTO source_label(concept_id,label,ordinal)
SELECT h.concept_id,h.old_label,
       coalesce((SELECT max(s.ordinal)+1 FROM source_label s
                 WHERE s.concept_id=h.concept_id),0)
FROM history_names h WHERE h.old_label<>h.new_label
AND NOT EXISTS (SELECT 1 FROM source_label s
                WHERE s.concept_id=h.concept_id AND s.label=h.old_label);
UPDATE concept SET preferred_label=h.new_label
FROM history_names h WHERE concept.concept_id=h.concept_id
AND h.old_label<>h.new_label;

DROP TABLE history_parent_names;
DROP TABLE history_names;
COMMIT;
