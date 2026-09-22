-- These exact selectors are already contained by a range on the same concept.
BEGIN;

DELETE FROM source_selector
WHERE system_id = 'lcc'
  AND (concept_id, selector) IN (
    VALUES
      (326, 'TE298'),
      (5559, 'KDC610'),
      (10293, 'DH151'),
      (10661, 'DT330'),
      (11525, 'DH916.2'),
      (11529, 'DG992.3'),
      (11553, 'DR1313.11'),
      (11554, 'DR1317'),
      (11554, 'DR1318'),
      (11554, 'DR1319')
  );

COMMIT;
