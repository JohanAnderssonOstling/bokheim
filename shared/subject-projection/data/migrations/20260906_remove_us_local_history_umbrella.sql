-- State and regional leaves already own the official subdivisions of F1-F975.
-- Keeping this umbrella range on United States makes those subdivisions overlap.
DELETE FROM source_selector
WHERE system_id = 'lcc'
  AND selector = 'F1..F975@0'
  AND concept_id IN (
    SELECT concept_id
    FROM concept
    WHERE preferred_label = 'United States'
  );
