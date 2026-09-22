-- Nest unambiguous national literature subperiods under century buckets.
-- Mixed or historically ambiguous ranges remain at their current level.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- English: 1900-2000 under the 20th century; post-2000 under the 21st.
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3913,4179,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4179),-1)+1;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3914,4179,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4179),-1)+1;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 3916,4180,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4180),-1)+1;
DELETE FROM concept_parent WHERE (concept_id=3913 AND parent_concept_id=5585)
   OR (concept_id=3914 AND parent_concept_id=5585)
   OR (concept_id=3916 AND parent_concept_id=5585);

-- German: 1860/70-2000 under the 20th century; post-2000 under the 21st.
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 12704,4190,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4190),-1)+1;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 12705,4190,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4190),-1)+1;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 4246,4191,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4191),-1)+1;
DELETE FROM concept_parent WHERE (concept_id=12704 AND parent_concept_id=4244)
   OR (concept_id=12705 AND parent_concept_id=4244)
   OR (concept_id=4246 AND parent_concept_id=4244);

-- Spanish: 1961-2000 under the 20th century; post-2000 under the 21st.
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 12716,4228,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4228),-1)+1;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 12717,4229,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4229),-1)+1;
DELETE FROM concept_parent WHERE (concept_id=12716 AND parent_concept_id=4226)
   OR (concept_id=12717 AND parent_concept_id=4226);

-- Russian: 1917-2000 under the 20th century; post-2000 under the 21st.
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 12494,4218,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4218),-1)+1;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 12495,4218,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4218),-1)+1;
INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT 12496,4219,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4219),-1)+1;
DELETE FROM concept_parent WHERE (concept_id=12494 AND parent_concept_id=4216)
   OR (concept_id=12495 AND parent_concept_id=4216)
   OR (concept_id=12496 AND parent_concept_id=4216);

COMMIT;
