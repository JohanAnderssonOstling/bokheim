-- Merge duplicate/related Romance literature nodes into country subjects.
-- Keep the BISAC country concepts as canonical where available, and transfer
-- the LCC ranges and detailed children from the related-literature concepts.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE romance_merges (source_id INTEGER PRIMARY KEY, target_id INTEGER NOT NULL);
INSERT INTO romance_merges VALUES
  (4238,4184), -- French & Related Literatures -> France
  (4258,4202), -- Italian -> Italy
  (4452,4215), -- Portuguese -> Portugal
  (4470,4226); -- Spanish -> Spain

INSERT OR IGNORE INTO lcc_selector (concept_id,selector)
SELECT m.target_id,l.selector
FROM romance_merges m JOIN lcc_selector l ON l.concept_id=m.source_id;
INSERT OR IGNORE INTO bisac_selector (concept_id,selector)
SELECT m.target_id,b.selector
FROM romance_merges m JOIN bisac_selector b ON b.concept_id=m.source_id;
INSERT OR IGNORE INTO ddc_selector (concept_id,selector)
SELECT m.target_id,d.selector
FROM romance_merges m JOIN ddc_selector d ON d.concept_id=m.source_id;

CREATE TEMP TABLE romance_children (source_id INTEGER,child_id INTEGER,PRIMARY KEY(source_id,child_id));
INSERT INTO romance_children
SELECT m.source_id,cp.concept_id
FROM romance_merges m JOIN concept_parent cp ON cp.parent_concept_id=m.source_id;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT child_id FROM romance_children)
   OR concept_id IN (SELECT source_id FROM romance_merges)
   OR parent_concept_id IN (SELECT source_id FROM romance_merges);

INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT child_id,target_id,0 FROM romance_children JOIN romance_merges USING (source_id);

DELETE FROM lcc_selector WHERE concept_id IN (SELECT source_id FROM romance_merges);
DELETE FROM bisac_selector WHERE concept_id IN (SELECT source_id FROM romance_merges);
DELETE FROM ddc_selector WHERE concept_id IN (SELECT source_id FROM romance_merges);
DELETE FROM source_label WHERE concept_id IN (SELECT source_id FROM romance_merges);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM romance_merges);

DROP TABLE romance_children;
DROP TABLE romance_merges;
COMMIT;
