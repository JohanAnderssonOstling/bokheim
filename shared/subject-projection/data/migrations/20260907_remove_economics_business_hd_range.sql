-- Bare HD is exactly owned by Industries.  The same-endpoint range on its
-- Economics & Business parent is redundant and violates unique ownership.

DELETE FROM lcc_range
WHERE concept_id=5104
  AND start_code='HD'
  AND end_code='HD';
