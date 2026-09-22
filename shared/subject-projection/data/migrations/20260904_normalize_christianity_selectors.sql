-- Selectors belong to the most specific retained Christianity concept. Parent
-- concepts remain reachable through taxonomy ancestry and need no duplicates.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE (concept_id='unified:79f83052-ab3b-5950-94d8-de07af5d1258'
       AND selector IN ('REL003000','REL043000','REL044000','REL079000','REL082000','REL088000'))
   OR (concept_id='bokheim:subject:christian-living:prayer-devotion'
       AND selector IN ('REL012020','REL012080','REL012150'))
   OR (concept_id='bokheim:subject:christian-living:spiritual-formation'
       AND selector IN ('REL012040','REL012070','REL012120','REL099000'))
   OR (concept_id='bokheim:christian-theology:christ-salvation'
       AND selector IN ('REL067040','REL067100','REL104000'))
   OR (concept_id='bokheim:christian-theology:church-ethics'
       AND selector IN ('REL067070','REL067120'))
   OR (concept_id='bokheim:christian-theology:doctrine-systematic'
       AND selector='REL067110');

-- Remove BR history, controversy, and regional ranges from the unrelated
-- "special subjects" umbrella.
DELETE FROM source_selector
WHERE concept_id='unified:130fdfab-00c4-512f-acf9-13b0a1e3c853'
  AND selector IN (
      'BR102.A1..BR102.Z@4','BR130..BR133.5@2','BR130..BR133.5@4',
      'BR1609.5..BR1609.5@2','BR1609.5..BR1609.5@4',
      'BR1614.92..BR1617.2@2','BR1614.92..BR1617.2@4',
      'BR1620..BR1620@2','BR1620..BR1620@4',
      'BR1690..BR1725@2','BR1690..BR1725@4',
      'BR1716.A1..BR1716.Z@4','BR1718.A1..BR1718.Z@4',
      'BR1719.A1..BR1719.Z@4'
  );

-- The restored leaf concepts now own these ranges.
DELETE FROM source_selector
WHERE (concept_id='bokheim:subject:christian-history:controversy-dissent-toleration'
       AND selector IN (
           'BR1609.5..BR1609.5@2','BR1609.5..BR1609.5@4','BR1610..BR1610@2',
           'BR1614.92..BR1617.2@2','BR1614.92..BR1617.2@4',
           'BR1620..BR1620@2','BR1620..BR1620@4'
       ))
   OR (concept_id='unified:45f8c80a-fad7-5002-a727-9703ff5348e9'
       AND selector IN ('BR60..BR67.2@2','BR323.492..BR334.2@5'));

COMMIT;
