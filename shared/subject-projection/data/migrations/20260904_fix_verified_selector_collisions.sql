-- Resolve selector collisions against the Library of Congress schedules.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- DT223-223.2 is Libyan ethnography; DT433.542-433.545 is Kenyan
-- ethnography. Neither belongs to the neighboring country.
DELETE FROM source_selector
WHERE (selector='DT223..DT223.2@4' AND concept_id='unified:12cbc983-844a-56a4-a344-dbb1de18d7ef')
   OR (selector='DT433.542..DT433.545@4' AND concept_id='unified:e3ad1bf3-e78a-5b6a-b5f3-32a22be71da3');

-- These DK ranges name individual Caucasus and Central Asian countries,
-- not Eastern Europe as an additional destination.
DELETE FROM source_selector
WHERE concept_id='unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec'
  AND selector IN (
      'DK670..DK679.5@4','DK680..DK689.5@4','DK690..DK699.5@4',
      'DK901..DK909.5@4','DK911..DK919.5@4','DK921..DK929.5@4',
      'DK931..DK939.5@4','DK941..DK949.5@4'
  );

-- KBM1846.5 is Jewish scribes (Soferim), not courts or proceedings.
DELETE FROM source_selector
WHERE selector='KBM1846.5..KBM1846.5@4'
  AND concept_id IN (
      'bokheim:law:procedure:courts-judiciary',
      'bokheim:law:procedure:specialized'
  );

-- Prices and price control belong to economic/industry regulation.
DELETE FROM source_selector
WHERE selector IN ('KBP3210..KBP3210@4','KJC6442..KJC6442@4','KJE864..KJE865@4')
  AND concept_id<>'bokheim:subject:law:business:regulation';

-- KJE6353-6361 and KJE6405 are European cultural-affairs provisions.
-- Retain their European integration mapping, not synthetic profession or
-- technology mappings.
DELETE FROM source_selector
WHERE selector IN ('KJE6353..KJE6361@4','KJE6405..KJE6405@4')
  AND concept_id<>'unified:120d8171-c5e5-54b9-a8d3-181886d2eb65';

-- KJE6554 is standards, norms, and quality control.
DELETE FROM source_selector
WHERE selector='KJE6554..KJE6554@4'
  AND concept_id<>'bokheim:subject:law:business:regulation';

-- PR81-98 is history of English literature. Remove the erroneous literary
-- form mappings and retain the range under literary history and criticism.
DELETE FROM source_selector
WHERE selector='PR81..PR98@4';

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:3594067d-ad6a-5a04-b0aa-5da2a7e3e1cd','lcc','PR81..PR98@4',
       COALESCE(MAX(ordinal),-1)+1
FROM source_selector
WHERE concept_id='unified:3594067d-ad6a-5a04-b0aa-5da2a7e3e1cd'
  AND system_id='lcc';

COMMIT;
