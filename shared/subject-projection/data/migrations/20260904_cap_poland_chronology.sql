-- Cap Poland at two chronological levels. Preserve the November and January
-- Uprisings as direct leaves while removing rulers and operational detail.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE poland_root(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO poland_root VALUES ('unified:b39c446a-3154-521c-8bf4-fa9c69c84ea5');

CREATE TEMP TABLE retained_poland(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO retained_poland VALUES
-- Broad periods.
('bokheim:history-lcc:a21dfefc-349d-51b9-bc43-bbff88f90d24'),
('bokheim:history-lcc:371b548e-448f-538d-aa71-4957ad773560'),
('bokheim:history-lcc:2c3d2953-dea5-5d2c-8283-62e830af1256'),
('bokheim:history-lcc:1bbadec6-7bfd-5b7e-80c4-ce1ac09cb331'),
('bokheim:history-lcc:ac8b706d-47d8-57a9-ae5e-2a46658936db'),
-- To 1795.
('bokheim:history-lcc-full:1462a588-41fe-58d9-8fdc-ea386e2cbfe2'),
('bokheim:history-lcc-full:1eb9955d-7cba-5ab5-be79-162778819b25'),
('bokheim:history-lcc-full:5e4c164c-b331-5728-9133-c9ea71dfbfd8'),
-- 1795-1918.
('bokheim:history-lcc-full:1392f956-1225-5747-ad5f-c73d3ebcecca'),
('bokheim:history-lcc-full:fe0c1b47-cc51-5867-8d53-936fcd9137a7'),
('bokheim:history-lcc-full:b42690e5-1e44-59ff-b965-6fda81cb565f'),
('bokheim:history-lcc-full:2a212efb-aa8d-5b2b-9db9-a69521ab2f7b'),
-- 1918-1945.
('bokheim:history-lcc-full:832e8157-4c2a-5041-bb0d-7e88ce8b48ca'),
('bokheim:history-lcc-full:ac8472fb-d1c1-5721-a97f-041c69d4ac95'),
('bokheim:history-lcc-full:4adeb4db-9701-5723-829e-aa59a09e8e3b'),
-- People's Republic.
('bokheim:history-lcc-full:540a2d41-b660-5d41-9033-7529d0e03d2a'),
('bokheim:history-lcc-full:62c65898-bd34-5103-bb0b-0a0c6d575719'),
('bokheim:history-lcc-full:76647c11-6e6a-5cc3-9b86-e32701b56cd3');

CREATE TEMP TABLE terminal_poland(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO terminal_poland
SELECT concept_id FROM retained_poland
WHERE concept_id NOT IN (
    'bokheim:history-lcc:a21dfefc-349d-51b9-bc43-bbff88f90d24',
    'bokheim:history-lcc:371b548e-448f-538d-aa71-4957ad773560',
    'bokheim:history-lcc:2c3d2953-dea5-5d2c-8283-62e830af1256',
    'bokheim:history-lcc:1bbadec6-7bfd-5b7e-80c4-ce1ac09cb331'
)
UNION SELECT 'bokheim:history-lcc:ac8b706d-47d8-57a9-ae5e-2a46658936db';

-- Move the two nationally significant uprisings directly under 1795-1918.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT uprising.concept_id,
       'bokheim:history-lcc:371b548e-448f-538d-aa71-4957ad773560',
       COALESCE((SELECT max(existing.ordinal)+1 FROM concept_parent existing WHERE existing.concept_id=uprising.concept_id),0),
       CASE uprising.concept_id
           WHEN 'bokheim:history-lcc-full:b42690e5-1e44-59ff-b965-6fda81cb565f' THEN 120
           ELSE 130
       END
FROM retained_poland uprising
WHERE uprising.concept_id IN (
    'bokheim:history-lcc-full:b42690e5-1e44-59ff-b965-6fda81cb565f',
    'bokheim:history-lcc-full:2a212efb-aa8d-5b2b-9db9-a69521ab2f7b'
)
AND NOT EXISTS (
    SELECT 1 FROM concept_parent existing
    WHERE existing.concept_id=uprising.concept_id
      AND existing.parent_concept_id='bokheim:history-lcc:371b548e-448f-538d-aa71-4957ad773560'
);

-- Reassign selectors from removed descendants to their nearest retained period.
CREATE TEMP TABLE selector_target(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
WITH RECURSIVE mapped(source_id,target_id) AS (
    SELECT concept_id,concept_id FROM retained_poland
    UNION
    SELECT child.concept_id,mapped.target_id
    FROM mapped
    JOIN concept_parent child ON child.parent_concept_id=mapped.source_id
    WHERE child.concept_id NOT IN (SELECT concept_id FROM retained_poland)
)
INSERT OR IGNORE INTO selector_target SELECT source_id,target_id FROM mapped;

CREATE TEMP TABLE selector_copy AS
SELECT DISTINCT target.target_id,source.system_id,source.selector
FROM selector_target target
JOIN source_selector source ON source.concept_id=target.source_id
WHERE target.source_id<>target.target_id
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id=target.target_id
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT copy.target_id,copy.system_id,copy.selector,
       COALESCE((SELECT max(existing.ordinal)+1 FROM source_selector existing
                 WHERE existing.concept_id=copy.target_id AND existing.system_id=copy.system_id),0)
       + row_number() OVER (PARTITION BY copy.target_id,copy.system_id ORDER BY copy.selector)-1
FROM selector_copy copy;

CREATE TEMP TABLE poland_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id
    FROM retained_poland retained
    JOIN concept_parent child ON child.parent_concept_id=retained.concept_id
    WHERE child.concept_id NOT IN (SELECT concept_id FROM retained_poland)
    UNION
    SELECT child.concept_id
    FROM detail parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id NOT IN (SELECT concept_id FROM retained_poland)
)
INSERT INTO poland_detail SELECT concept_id FROM detail;

-- Retained second-level entries are leaves.
DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM terminal_poland);

CREATE TEMP TABLE shared_poland_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
    SELECT detail.concept_id
    FROM poland_detail detail
    JOIN concept_parent incoming ON incoming.concept_id=detail.concept_id
    WHERE incoming.parent_concept_id NOT IN (SELECT concept_id FROM poland_detail)
      AND incoming.parent_concept_id NOT IN (SELECT concept_id FROM retained_poland)
      AND incoming.parent_concept_id NOT IN (SELECT concept_id FROM poland_root)
    UNION
    SELECT child.concept_id
    FROM shared parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id IN (SELECT concept_id FROM poland_detail)
)
INSERT INTO shared_poland_detail SELECT concept_id FROM shared;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM poland_detail WHERE concept_id NOT IN (SELECT concept_id FROM shared_poland_detail))
   OR parent_concept_id IN (SELECT concept_id FROM poland_detail WHERE concept_id NOT IN (SELECT concept_id FROM shared_poland_detail));
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM poland_detail WHERE concept_id NOT IN (SELECT concept_id FROM shared_poland_detail));

UPDATE concept SET preferred_label='To 1795'
WHERE concept_id='bokheim:history-lcc:a21dfefc-349d-51b9-bc43-bbff88f90d24';
UPDATE concept SET preferred_label='1795–1918'
WHERE concept_id='bokheim:history-lcc:371b548e-448f-538d-aa71-4957ad773560';
UPDATE concept SET preferred_label='1945–1989: People''s Republic'
WHERE concept_id='bokheim:history-lcc:1bbadec6-7bfd-5b7e-80c4-ce1ac09cb331';
UPDATE concept SET preferred_label='1989–Present'
WHERE concept_id='bokheim:history-lcc:ac8b706d-47d8-57a9-ae5e-2a46658936db';
UPDATE concept SET preferred_label='1572–1763: Elective Monarchy'
WHERE concept_id='bokheim:history-lcc-full:1eb9955d-7cba-5ab5-be79-162778819b25';
UPDATE concept SET preferred_label='1763–1795: Partition Period'
WHERE concept_id='bokheim:history-lcc-full:5e4c164c-b331-5728-9133-c9ea71dfbfd8';
UPDATE concept SET preferred_label='November Uprising, 1830–1831'
WHERE concept_id='bokheim:history-lcc-full:b42690e5-1e44-59ff-b965-6fda81cb565f';
UPDATE concept SET preferred_label='January Uprising, 1863–1864'
WHERE concept_id='bokheim:history-lcc-full:2a212efb-aa8d-5b2b-9db9-a69521ab2f7b';
UPDATE concept SET preferred_label='1939–1945'
WHERE concept_id='bokheim:history-lcc-full:4adeb4db-9701-5723-829e-aa59a09e8e3b';

DROP TABLE shared_poland_detail;
DROP TABLE poland_detail;
DROP TABLE selector_copy;
DROP TABLE selector_target;
DROP TABLE terminal_poland;
DROP TABLE retained_poland;
DROP TABLE poland_root;
COMMIT;
