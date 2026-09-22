-- Replace enumerated Family Law selectors with the broad schedule spans
-- explicitly identified as domestic relations/family law by the official LCC
-- definitions. Family Law is a leaf, so these permissive ranges are allowed.
BEGIN;

DELETE FROM source_selector
WHERE concept_id = 5288
  AND system_id = 'lcc'
  AND (
    selector GLOB 'KEQ2*' OR
    selector GLOB 'KIA3*' OR
    selector GLOB 'KIA2*' OR
    selector GLOB 'KJV6*' OR
    selector GLOB 'KJV7*' OR
    selector GLOB 'KJV8*' OR
    selector GLOB 'KK1*' OR
    selector GLOB 'KKA1*'
  );

INSERT INTO source_selector(concept_id, system_id, selector) VALUES
  (5288, 'lcc', 'KEQ237..KEQ258'),
  (5288, 'lcc', 'KIA35..KIA38'),
  (5288, 'lcc', 'KIA237..KIA247'),
  (5288, 'lcc', 'KIA2050..KIA2067'),
  -- KJA2227..KJA2269 is also assigned to the Roman-law history leaf.
  -- Keep its family-law schedule points exact until that collision is split.
  (5288, 'lcc', 'KJV622..KJV861.2'),
  (5288, 'lcc', 'KK1100..KK1248'),
  (5288, 'lcc', 'KKA1100.5..KKA1213');

COMMIT;
