-- Replace the Social Sciences root's H-HX/H1-H99 catch-alls with official,
-- high-use general-H schedule partitions on existing leaves.  Every named H
-- subclass already has a more specific owner; HE was the sole misplaced bare
-- subclass selector.

UPDATE lcc_selector SET concept_id=488
WHERE concept_id=5055 AND selector='HE';

DELETE FROM lcc_range
WHERE concept_id=5055
  AND (start_code,end_code) IN (
    ('H','HX'),
    ('H1','H99')
  );

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('H1','H29',12245),
  ('H40','H50',12245),
  ('H61','H61.95',5509),
  ('H62','H62.5',52939),
  ('H96','H98',5374);
