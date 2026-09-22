-- Compact Norway's early-period and local-history enumerations into official
-- leaf ranges. Existing broad chronological leaves retain the later periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _norway_base(id INTEGER NOT NULL);
INSERT INTO _norway_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _norway_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _norway_node VALUES
 (1,'Early Norway, to 1035','DL460..DL468.5'),
 (2,'Counties & Regions','DL576.A..DL576.Z'),
 (3,'Cities & Towns','DL581..DL596.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _norway_node n CROSS JOIN _norway_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3583,n.seq FROM _norway_node n CROSS JOIN _norway_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _norway_node n CROSS JOIN _norway_base b;

-- Remove the former early-period umbrellas and local cutter enumeration.
DELETE FROM source_selector WHERE concept_id=3583 AND system_id='lcc'
 AND (selector IN ('DL460..DL478','DL462..DL464','DL581')
      OR selector GLOB 'DL576.*' OR selector GLOB 'DL596.*');

-- Remove restored parent copies already owned exactly by a direct era child.
DELETE FROM source_selector AS parent
WHERE parent.concept_id=3583 AND parent.system_id='lcc'
 AND EXISTS (
   SELECT 1 FROM concept_parent cp
   JOIN source_selector child ON child.concept_id=cp.concept_id
   WHERE cp.parent_concept_id=3583
     AND child.system_id='lcc' AND child.selector=parent.selector
 );

DROP TABLE _norway_node;
DROP TABLE _norway_base;
COMMIT;
