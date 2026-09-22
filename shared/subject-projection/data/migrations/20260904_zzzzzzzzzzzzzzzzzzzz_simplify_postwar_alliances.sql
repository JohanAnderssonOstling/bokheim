-- Remove redundant postwar alliance levels while keeping period-specific
-- Soviet Bloc concepts distinct.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- NATO directly represents both the treaty and the organization.
UPDATE concept_parent
SET parent_concept_id='bokheim:lcc-curated:b38b023d-79b3-503b-924d-97174efa6561',
    display_order=10
WHERE concept_id='bokheim:lcc-curated:adebc028-c4fd-5086-baea-0e7ef1323e14'
  AND parent_concept_id='bokheim:lcc-curated:316d6b25-6776-580e-9132-47f8cbf0b8ea';

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:lcc-curated:adebc028-c4fd-5086-baea-0e7ef1323e14',
       system_id,selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id='bokheim:lcc-curated:adebc028-c4fd-5086-baea-0e7ef1323e14'
                   AND existing.system_id=source_selector.system_id),0)
FROM source_selector
WHERE concept_id='bokheim:lcc-curated:316d6b25-6776-580e-9132-47f8cbf0b8ea';

DELETE FROM concept
WHERE concept_id='bokheim:lcc-curated:316d6b25-6776-580e-9132-47f8cbf0b8ea';

-- Merge the duplicate 1945-1965 Soviet-bloc heading into its curated sibling.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:lcc-curated:a86d3dad-9572-55e3-a692-09be82627886',
       system_id,selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id='bokheim:lcc-curated:a86d3dad-9572-55e3-a692-09be82627886'
                   AND existing.system_id=source_selector.system_id),0)
FROM source_selector
WHERE concept_id='bokheim:history-lcc-full:f1f7b0ac-9a9f-5dd2-928e-7e8216ed6e29';

DELETE FROM concept
WHERE concept_id='bokheim:history-lcc-full:f1f7b0ac-9a9f-5dd2-928e-7e8216ed6e29';

COMMIT;
