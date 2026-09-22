-- Merge duplicate Recreation > Indoor games and amusements concepts so the
-- curated graph retains one node carrying both DDC and LCC selectors.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:b0181b55-6b2e-5347-87f6-672b3127ff37',source.system_id,source.selector,
       COALESCE((SELECT max(ordinal)+1 FROM source_selector
                 WHERE concept_id='unified:b0181b55-6b2e-5347-87f6-672b3127ff37'
                   AND system_id=source.system_id),0)
       + row_number() OVER (PARTITION BY source.system_id ORDER BY source.ordinal)-1
FROM source_selector source
WHERE source.concept_id='unified:73b909e5-fda7-59a6-8ac9-b4fa65d5cd9e'
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id='unified:b0181b55-6b2e-5347-87f6-672b3127ff37'
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

DELETE FROM concept_parent WHERE concept_id='unified:73b909e5-fda7-59a6-8ac9-b4fa65d5cd9e';
DELETE FROM concept WHERE concept_id='unified:73b909e5-fda7-59a6-8ac9-b4fa65d5cd9e';
COMMIT;
