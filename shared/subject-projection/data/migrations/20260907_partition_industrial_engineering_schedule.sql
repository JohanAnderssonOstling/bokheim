-- Partition T55.4-T60.8 around the existing specialist leaves. Preserve the
-- printed whole division exactly on Industrial Engineering.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53365,'Industrial Engineering Reference & Study');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53365,446,0);

DELETE FROM lcc_range WHERE concept_id=446;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('T55.4','T55.69',53365),
  ('T55.86','T56.29',53365),
  ('T56.4','T56.79',53365);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (446,'T55.4-60.8'),
  (446,'T58'),
  (446,'T58.4'),
  (446,'T59.6');

-- Operations research is the more specific owner of T57.6-T57.97.
UPDATE lcc_range
SET end_code='T57.59'
WHERE concept_id=6411
  AND start_code='T57'
  AND end_code='T57.97';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (6411,'T57-57.97');

-- Management information systems is the more specific T58.6-T58.62 owner.
DELETE FROM lcc_range
WHERE concept_id=7640
  AND start_code='T58.5'
  AND end_code='T58.64';
INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('T58.5','T58.59',7640),
  ('T58.63','T58.64',7640);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (7640,'T58.5-58.64');

-- Already contained by the numeric Industrial Safety range.
DELETE FROM lcc_range
WHERE concept_id=448
  AND start_code='T55.A2'
  AND end_code='T55.Z';
