-- Put country-specific genre nodes directly under their national literature
-- concepts, matching the German treatment.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Drama' WHERE concept_id IN (3883,3887,3889,3881,3906);
UPDATE concept SET preferred_label='Poetry' WHERE concept_id IN (4427,4434,4436,3930);

DELETE FROM concept_parent
WHERE (concept_id=3883 AND parent_concept_id=3876)
   OR (concept_id=3887 AND parent_concept_id=3885)
   OR (concept_id=3889 AND parent_concept_id=3885)
   OR (concept_id=3881 AND parent_concept_id=3876)
   OR (concept_id=3906 AND parent_concept_id=3911)
   OR (concept_id=4427 AND parent_concept_id=4425)
   OR (concept_id=4434 AND parent_concept_id=4432)
   OR (concept_id=4436 AND parent_concept_id=4432)
   OR (concept_id=3930 AND parent_concept_id=3911);

INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3883,4161,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=4161;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3887,4184,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=4184;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3889,4202,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=4202;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3881,4268,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=4268;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3906,5585,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=5585;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 4427,4268,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=4268;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 4434,4184,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=4184;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 4436,4202,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=4202;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3930,5585,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=5585;

INSERT OR IGNORE INTO lcc_selector (concept_id,selector) VALUES
  (4434,'PQ400-PQ491'),
  (3887,'PQ500-PQ591'),
  (4436,'PQ4091-PQ4131'),
  (3889,'PQ4133-PQ4160'),
  (3930,'PR500-PR614'),
  (3906,'PR621-PR744');

COMMIT;
