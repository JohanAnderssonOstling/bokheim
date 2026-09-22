-- Broad LCC fallbacks must point only to a subject that covers the complete
-- class. B and P span multiple curated branches and therefore have no safe
-- single destination. H and J do have broad curated ancestors.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND selector IN ('B*','P*');

DELETE FROM source_selector
WHERE system_id='lcc' AND selector IN ('H*','J*');

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:f8f652e6-6244-5dc5-97b7-9dece2de275f','lcc','H*',
       COALESCE((SELECT max(ordinal)+1 FROM source_selector
                 WHERE concept_id='unified:f8f652e6-6244-5dc5-97b7-9dece2de275f' AND system_id='lcc'),0);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:fa671db5-b0ef-5492-9c50-638c60fa5148','lcc','J*',
       COALESCE((SELECT max(ordinal)+1 FROM source_selector
                 WHERE concept_id='unified:fa671db5-b0ef-5492-9c50-638c60fa5148' AND system_id='lcc'),0);

COMMIT;
