-- Promote the two high-use city subdivisions from Russia's broad fallback
-- range. General Russian topical and regional codes remain at Russia.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:history:russia:moscow','Moscow'),
 ('bokheim:history:russia:saint-petersburg','Saint Petersburg');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:history:russia:moscow','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10',0,50),
 ('bokheim:history:russia:saint-petersburg','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10',0,60);

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:history:russia:moscow','lcc','DK588..DK609@5',0),
 ('bokheim:history:russia:saint-petersburg','lcc','DK541..DK579@5',0);

COMMIT;
