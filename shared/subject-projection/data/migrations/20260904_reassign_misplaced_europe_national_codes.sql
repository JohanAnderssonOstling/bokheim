-- Move national LCC selectors out of regional period trees without changing browsing structure.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE national_move(source_id TEXT,system_id TEXT,selector TEXT,target_id TEXT,PRIMARY KEY(source_id,system_id,selector)) WITHOUT ROWID;

WITH RECURSIVE northern(id) AS (
 SELECT p.concept_id FROM concept r JOIN concept_parent p ON p.parent_concept_id=r.concept_id JOIN concept c ON c.concept_id=p.concept_id
 WHERE r.preferred_label='Northern Europe' AND lower(c.preferred_label)='by period'
 UNION SELECT p.concept_id FROM northern n JOIN concept_parent p ON p.parent_concept_id=n.id
), balkans(id) AS (
 SELECT p.concept_id FROM concept r JOIN concept_parent p ON p.parent_concept_id=r.concept_id JOIN concept c ON c.concept_id=p.concept_id
 WHERE r.preferred_label='History of Balkan Peninsula' AND lower(c.preferred_label)='by period'
 UNION SELECT p.concept_id FROM balkans b JOIN concept_parent p ON p.parent_concept_id=b.id
)
INSERT INTO national_move
SELECT s.concept_id,s.system_id,s.selector,
 CASE
  WHEN s.selector GLOB 'DL19*' OR s.selector GLOB 'DL2*' THEN 'unified:4a84d354-9db9-5152-9897-c3af9dbcfa57'
  WHEN s.selector GLOB 'DL525*' THEN 'unified:0fbb9742-b8c8-52d7-8d1d-aa88c39ee3fc'
  WHEN s.selector GLOB 'DL733*' OR s.selector GLOB 'DL790*' OR s.selector GLOB 'DL805*' THEN 'unified:f026602f-cc16-5617-bba6-5fa90f489b0d'
 END
FROM northern n JOIN source_selector s ON s.concept_id=n.id
WHERE s.system_id='lcc' AND (s.selector GLOB 'DL19*' OR s.selector GLOB 'DL2*' OR s.selector GLOB 'DL525*' OR s.selector GLOB 'DL733*' OR s.selector GLOB 'DL790*' OR s.selector GLOB 'DL805*')
UNION ALL
SELECT s.concept_id,s.system_id,s.selector,'bokheim:history-lcc:8fad3838-2273-5eff-af3d-c0b64bd35c0c'
FROM balkans b JOIN source_selector s ON s.concept_id=b.id
WHERE s.system_id='lcc' AND (s.selector GLOB 'DR12*' OR s.selector GLOB 'DR13*');

WITH numbered AS (
 SELECT m.*,row_number() OVER (PARTITION BY target_id,system_id ORDER BY selector)-1 rn
 FROM national_move m WHERE NOT EXISTS (SELECT 1 FROM source_selector s WHERE s.concept_id=m.target_id AND s.system_id=m.system_id AND s.selector=m.selector)
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,COALESCE((SELECT max(s.ordinal)+1 FROM source_selector s WHERE s.concept_id=numbered.target_id AND s.system_id=numbered.system_id),0)+rn FROM numbered;

DELETE FROM source_selector
WHERE (concept_id,system_id,selector) IN (SELECT source_id,system_id,selector FROM national_move);

DROP TABLE national_move;
COMMIT;
