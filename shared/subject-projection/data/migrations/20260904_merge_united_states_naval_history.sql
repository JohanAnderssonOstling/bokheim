-- Merge the separate United States naval-history category into military
-- history in the curated taxonomy. Full source detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO source_label(concept_id,label,ordinal)
SELECT 'bokheim:lcc-detail:eb47e777-f6b4-527c-bca1-33c83b2989d9',source.label,
       COALESCE((SELECT max(ordinal) FROM source_label
                 WHERE concept_id='bokheim:lcc-detail:eb47e777-f6b4-527c-bca1-33c83b2989d9'),-1)
       + row_number() OVER (ORDER BY source.ordinal,source.label)
FROM source_label source
WHERE source.concept_id='bokheim:lcc-detail:bfe15810-515f-59ce-b549-3c1be999f501'
  AND NOT EXISTS (
      SELECT 1 FROM source_label existing
      WHERE existing.concept_id='bokheim:lcc-detail:eb47e777-f6b4-527c-bca1-33c83b2989d9'
        AND existing.label=source.label
  );

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:lcc-detail:eb47e777-f6b4-527c-bca1-33c83b2989d9',source.system_id,source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector
                 WHERE concept_id='bokheim:lcc-detail:eb47e777-f6b4-527c-bca1-33c83b2989d9'
                   AND system_id=source.system_id),-1)
       + row_number() OVER (PARTITION BY source.system_id ORDER BY source.ordinal,source.selector)
FROM source_selector source
WHERE source.concept_id='bokheim:lcc-detail:bfe15810-515f-59ce-b549-3c1be999f501'
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id='bokheim:lcc-detail:eb47e777-f6b4-527c-bca1-33c83b2989d9'
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

DELETE FROM concept_parent
WHERE concept_id='bokheim:lcc-detail:bfe15810-515f-59ce-b549-3c1be999f501'
   OR parent_concept_id='bokheim:lcc-detail:bfe15810-515f-59ce-b549-3c1be999f501';
DELETE FROM concept
WHERE concept_id='bokheim:lcc-detail:bfe15810-515f-59ce-b549-3c1be999f501';

COMMIT;
