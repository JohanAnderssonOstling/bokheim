-- Russia is intentionally broad in the curated taxonomy.  The exhaustive
-- locality selectors are wholly covered by these two LCC schedule ranges.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id='unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'
  AND system_id='lcc';

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('unified:55b70b6c-2304-5d17-80d5-c4c687b80d10','lcc','DK510..DK651@4',0),
 ('unified:55b70b6c-2304-5d17-80d5-c4c687b80d10','lcc','DK751..DK781@4',1);

COMMIT;
