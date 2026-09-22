-- Consolidate Mexico's overlapping curated period entries into one continuous
-- chronology. The source-specific detail remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Preserve both source ranges on the combined 1849-1867 concept.
INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT 'bokheim:history-lcc-full:acebcf04-2ebb-5d99-8323-43e2299cf746',
       source.system_id,
       source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector
                 WHERE concept_id='bokheim:history-lcc-full:acebcf04-2ebb-5d99-8323-43e2299cf746'
                   AND system_id=source.system_id), -1)
       + row_number() OVER (PARTITION BY source.system_id ORDER BY source.ordinal)
FROM source_selector source
WHERE source.concept_id='bokheim:history-lcc-full:17dbc623-8ed0-52f7-ade3-fd513b2a4df8'
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id='bokheim:history-lcc-full:acebcf04-2ebb-5d99-8323-43e2299cf746'
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

INSERT INTO source_label(concept_id, label, ordinal)
SELECT 'bokheim:history-lcc-full:acebcf04-2ebb-5d99-8323-43e2299cf746',
       source.label,
       COALESCE((SELECT max(ordinal) FROM source_label
                 WHERE concept_id='bokheim:history-lcc-full:acebcf04-2ebb-5d99-8323-43e2299cf746'), -1)
       + row_number() OVER (ORDER BY source.ordinal)
FROM source_label source
WHERE source.concept_id='bokheim:history-lcc-full:17dbc623-8ed0-52f7-ade3-fd513b2a4df8'
  AND NOT EXISTS (
      SELECT 1 FROM source_label existing
      WHERE existing.concept_id='bokheim:history-lcc-full:acebcf04-2ebb-5d99-8323-43e2299cf746'
        AND existing.label=source.label
  );

UPDATE concept
SET preferred_label='1849-1867'
WHERE concept_id='bokheim:history-lcc-full:acebcf04-2ebb-5d99-8323-43e2299cf746';

CREATE TEMP TABLE mexico_period_to_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO mexico_period_to_remove VALUES
    ('bokheim:history-lcc-full:6abbb865-a82b-586e-aae5-aafbb0bfa21e'),
    ('bokheim:history-lcc-full:a450d900-6ac5-5249-8f29-80ef310988ae'),
    ('bokheim:history-lcc-full:17dbc623-8ed0-52f7-ade3-fd513b2a4df8');

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM mexico_period_to_remove)
   OR parent_concept_id IN (SELECT concept_id FROM mexico_period_to_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM mexico_period_to_remove);

DROP TABLE mexico_period_to_remove;
COMMIT;
