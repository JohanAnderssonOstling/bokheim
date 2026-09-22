-- Remove two redundant general wrappers and partition the next two high-hit
-- schedules. General material remains at the surviving broad parent.
BEGIN;

CREATE TEMP TABLE _merge(source_id INTEGER PRIMARY KEY,target_id INTEGER NOT NULL);
INSERT INTO _merge VALUES
 (2191,2169),   -- duplicate nested Education -> Education root
 (13668,2707); -- General Internal Medicine -> Internal Medicine

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
UPDATE lcc_range
SET concept_id=(SELECT target_id FROM _merge WHERE source_id=lcc_range.concept_id)
WHERE concept_id IN(SELECT source_id FROM _merge);
DELETE FROM lcc_selector WHERE concept_id IN(SELECT source_id FROM _merge);
DELETE FROM bisac_selector WHERE concept_id IN(SELECT source_id FROM _merge);
DELETE FROM source_label WHERE concept_id IN(SELECT source_id FROM _merge);
DELETE FROM concept_parent WHERE concept_id IN(SELECT source_id FROM _merge)
 OR parent_concept_id IN(SELECT source_id FROM _merge);
DELETE FROM concept WHERE concept_id IN(SELECT source_id FROM _merge);
DROP TABLE _merge;

-- Education subclasses. General L and LC coverage stays at Education.
DELETE FROM lcc_range WHERE concept_id=2169
 AND ((start_code='L' AND end_code='LT') OR
      (start_code='LH1' AND end_code='LH9'));
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('L','L',2169),('LA','LA',2203),('LB','LB',2315),
 ('LC','LC',2169),('LD','LG',2211),('LH','LH',2252),
 ('LJ','LJ',2297),('LT','LT',2314);

-- Internal medicine. Named clinical divisions move to established children;
-- reference material and all true gaps remain at Internal Medicine.
DELETE FROM lcc_range WHERE concept_id=2707
 AND ((start_code='RC1' AND end_code='RC1245') OR
      (start_code='RC321' AND end_code='RC580'));
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RC1','RC70.99',2707),('RC71','RC78.7',2709),
 ('RC78.71','RC80.99',2707),('RC81','RC82',2714),
 ('RC82.1','RC85.99',2707),('RC86','RC88.9',2711),
 ('RC88.91','RC90.99',2707),('RC91','RC103',2708),
 ('RC103.1','RC108.99',2707),('RC109','RC216',2710),
 ('RC216.1','RC253.99',2707),('RC254','RC282',2712),
 ('RC282.1','RC305.99',2707),('RC306','RC320.5',2719),
 ('RC320.6','RC433.99',2707),('RC434.2','RC511.99',2837),
 ('RC512','RC569.5',2715),('RC569.6','RC574',2837),
 ('RC574.1','RC951.99',2707),('RC952','RC1245',2717);

COMMIT;
