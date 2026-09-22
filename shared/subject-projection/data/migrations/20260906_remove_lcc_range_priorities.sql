-- Apply to both curated and master format-4 taxonomies.
-- Expand the existing abbreviated endpoint without changing its intended bounds.
UPDATE source_selector SET selector='GR140..GR153@3' WHERE system_id='lcc' AND selector='GR140..153@3';
CREATE TEMP TABLE priority_free_ranges AS
SELECT DISTINCT concept_id,system_id,substr(selector,1,instr(selector,'@')-1) AS selector
FROM source_selector WHERE system_id='lcc' AND selector LIKE '%..%@%';
DELETE FROM source_selector WHERE system_id='lcc' AND selector LIKE '%..%@%';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT concept_id,system_id,selector FROM priority_free_ranges;
DROP TABLE priority_free_ranges;
