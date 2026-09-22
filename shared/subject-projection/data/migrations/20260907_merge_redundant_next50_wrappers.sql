-- Collapse redundant intermediate subjects from the next-50 review. Their
-- selectors belong to the immediate parent and their children remain direct,
-- named subjects. Real domain nodes in the candidate list are preserved.
BEGIN;

CREATE TEMP TABLE _merge(source_id INTEGER PRIMARY KEY,target_id INTEGER NOT NULL);
INSERT INTO _merge VALUES
 (656,650),    -- Other arts & art industries -> Decorative Arts
 (5851,915),   -- Techniques, Equipment & Processing -> Photography & Imaging
 (8788,3117),  -- Cookbooks -> Cooking
 (9610,259),   -- Apparatus & materials -> Electronics
 (12354,5234), -- Labor — General -> Labor, Work & Working Class
 (12807,5105); -- Accounting & Bookkeeping -> Accounting

-- Reparent the meaningful children. Allocate a free ordinal per child so
-- polyhierarchical children keep all their existing routes.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT p.concept_id,m.target_id,
       coalesce((SELECT max(x.ordinal)+1 FROM concept_parent x
                 WHERE x.concept_id=p.concept_id),0)
FROM concept_parent p JOIN _merge m ON m.source_id=p.parent_concept_id
WHERE NOT EXISTS (
  SELECT 1 FROM concept_parent x
  WHERE x.concept_id=p.concept_id AND x.parent_concept_id=m.target_id
);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT m.target_id,s.selector FROM lcc_selector s JOIN _merge m ON m.source_id=s.concept_id;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT m.target_id,s.selector FROM bisac_selector s JOIN _merge m ON m.source_id=s.concept_id;

-- Range endpoints have one structural owner; none of these endpoints collide
-- with their destination parent in the reviewed current database.
UPDATE lcc_range
SET concept_id=(SELECT target_id FROM _merge WHERE source_id=lcc_range.concept_id)
WHERE concept_id IN (SELECT source_id FROM _merge);

-- Preserve former labels as aliases at the surviving parent.
INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT m.target_id,c.preferred_label,
       coalesce((SELECT max(sl.ordinal)+1 FROM source_label sl
                 WHERE sl.concept_id=m.target_id),0)
       + row_number() OVER (PARTITION BY m.target_id ORDER BY c.preferred_label)-1
FROM _merge m JOIN concept c ON c.concept_id=m.source_id
WHERE NOT EXISTS (
 SELECT 1 FROM source_label sl
 WHERE sl.concept_id=m.target_id AND sl.label=c.preferred_label
);

-- Delete source-owned rows explicitly: migration runners are not required to
-- have SQLite foreign-key cascades enabled.
DELETE FROM lcc_selector WHERE concept_id IN (SELECT source_id FROM _merge);
DELETE FROM bisac_selector WHERE concept_id IN (SELECT source_id FROM _merge);
DELETE FROM source_label WHERE concept_id IN (SELECT source_id FROM _merge);
DELETE FROM concept_parent WHERE concept_id IN (SELECT source_id FROM _merge)
 OR parent_concept_id IN (SELECT source_id FROM _merge);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM _merge);
DROP TABLE _merge;

-- Accounting already had a direct Specialized Accounting child. Collapsing
-- Accounting & Bookkeeping exposes a second same-named child; retain the
-- established ranged concept and remove the empty duplicate.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 52584,selector FROM lcc_selector WHERE concept_id=53490;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 52584,selector FROM bisac_selector WHERE concept_id=53490;
UPDATE lcc_range SET concept_id=52584 WHERE concept_id=53490;
DELETE FROM lcc_selector WHERE concept_id=53490;
DELETE FROM bisac_selector WHERE concept_id=53490;
DELETE FROM source_label WHERE concept_id=53490;
DELETE FROM concept_parent WHERE concept_id=53490 OR parent_concept_id=53490;
DELETE FROM concept WHERE concept_id=53490;
COMMIT;
