-- Remove LCC ranges that are already wholly contained by another range on the
-- same concept.  This is representation-only compaction: it neither broadens
-- matching nor changes which concept owns any call number.
BEGIN;

DELETE FROM source_selector
WHERE system_id = 'lcc'
  AND (concept_id, selector) IN (
    VALUES
      (5712, 'K87..K88'),
      (5711, 'K120..K129'),
      (5708, 'KD1095..KD1098'),
      (5708, 'KD1102..KD1107'),
      (5708, 'KE5194..KE5196'),
      (5708, 'KE5201..KE5217'),
      (5708, 'KEO913..KEO914'),
      (5708, 'KEO915..KEO916'),
      (5708, 'KEQ858..KEQ859'),
      (5706, 'KJA2714..KJA2726'),
      (5706, 'KJA2720..KJA2772'),
      (5706, 'KJC3908..KJC3925.5'),
      (5706, 'KJC3926..KJC3927'),
      (5646, 'KJC7101..KJC7309'),
      (5646, 'KJC7312..KJC7346.2')
  );

COMMIT;
