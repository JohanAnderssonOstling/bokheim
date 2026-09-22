-- Compact high-selector leaf subjects only where the LCC schedule provides
-- one exact, semantically identical span.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Paris: official History of France / Local history and description span.
DELETE FROM source_selector
WHERE concept_id=12745 AND system_id='lcc'
  AND selector >= 'DC701' AND selector <= 'DC790.ZZZ';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
VALUES(12745,'lcc','DC701..DC790');

COMMIT;
