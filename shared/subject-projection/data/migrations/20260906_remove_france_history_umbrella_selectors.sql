-- France is a navigational chronology/region node.  Its permissive DC-wide
-- umbrella and the enumerated Paris/1940s selectors duplicate child ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id=3523 AND system_id='lcc' AND selector<>'DC';

-- Remove narrower ranges already wholly contained by the same child's broad
-- official span.
DELETE FROM source_selector
WHERE system_id='lcc' AND (
 (concept_id=3555 AND selector='DC95..DC109') OR
 (concept_id=9724 AND selector='DC124..DC124.45') OR
 (concept_id=10409 AND selector='DC218..DC219') OR
 (concept_id=11462 AND selector='DC340..DC341')
);

COMMIT;
