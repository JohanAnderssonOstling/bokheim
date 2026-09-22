-- Attach verified printed LCC range labels to existing subjects.
-- No new concepts, parent edges, permissive ranges or wildcards. Existing matches were preserved.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:f158bcdc-8f35-5cea-baa7-fce8bcbdc3b4','lcc','QA1-939',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:f158bcdc-8f35-5cea-baa7-fce8bcbdc3b4' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b','lcc','HF4999.2-6182',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:df4df9ce-4492-500d-a82d-1ce50c92f18b' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:subject:technology:general','lcc','TA1-2040',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:subject:technology:general' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:68cb5226-6697-5d3e-b247-e0bf9d5deafb','lcc','JA1-92',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:68cb5226-6697-5d3e-b247-e0bf9d5deafb' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:ba1ce9c4-e1f0-5660-829a-d836b81a894b','lcc','QH301-705',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:ba1ce9c4-e1f0-5660-829a-d836b81a894b' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:subject:technology:general','lcc','T1-995',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:subject:technology:general' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:17a7ea1c-4012-5d0d-afb4-9c6b194c2ea7','lcc','L1-991',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:17a7ea1c-4012-5d0d-afb4-9c6b194c2ea7' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:4887dbea-a685-5e73-8fdb-c776eb56ae3c','lcc','QD1-999',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:4887dbea-a685-5e73-8fdb-c776eb56ae3c' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:7810e0dc-ef10-56d2-aaca-1d41c342da12','lcc','GE1-350',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:7810e0dc-ef10-56d2-aaca-1d41c342da12' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:60797396-c27e-58ca-8f54-bf034abb25e4','lcc','Q334-342',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:60797396-c27e-58ca-8f54-bf034abb25e4' AND system_id='lcc';
COMMIT;
