BEGIN IMMEDIATE;

-- Replace the generated BP class bucket with the five mutually exclusive
-- divisions in the current LC outline. Reuse the existing adult BISAC Islam
-- and Baha'i concepts so each religion has one curated concept.

DELETE FROM source_selector
WHERE concept_id = 'unified:fa00571a-d187-5a8a-9dfa-d9046f4ed9c8';
DELETE FROM concept_parent
WHERE concept_id = 'unified:fa00571a-d187-5a8a-9dfa-d9046f4ed9c8';
DELETE FROM source_label
WHERE concept_id = 'unified:fa00571a-d187-5a8a-9dfa-d9046f4ed9c8';
DELETE FROM concept
WHERE concept_id = 'unified:fa00571a-d187-5a8a-9dfa-d9046f4ed9c8';

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order)
VALUES
  ('unified:1534a8df-3f1b-5058-8745-870315e0141e', 'bokheim:religion:other-movements', 1, 10),
  ('unified:ec547fd9-094e-54c4-b882-cbb20015b7f7', 'bokheim:religion:other-movements', 1, 20);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal)
VALUES
  ('unified:1534a8df-3f1b-5058-8745-870315e0141e', 'lcc', 'BP1..BP253@1', 0),
  ('unified:ec547fd9-094e-54c4-b882-cbb20015b7f7', 'lcc', 'BP300..BP420.5@1', 0);

INSERT INTO concept (concept_id, preferred_label)
VALUES
  ('bokheim:religion:theosophy', 'Theosophy'),
  ('bokheim:religion:anthroposophy', 'Anthroposophy'),
  ('bokheim:religion:other-beliefs-movements', 'Other Beliefs & Movements');

INSERT INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order)
VALUES
  ('bokheim:religion:theosophy', 'bokheim:religion:other-movements', 0, 30),
  ('bokheim:religion:anthroposophy', 'bokheim:religion:other-movements', 0, 40),
  ('bokheim:religion:other-beliefs-movements', 'bokheim:religion:other-movements', 0, 50);

-- REL068000 is specifically Theosophy, not the generic Religious Movements
-- parent on which it was previously parked.
DELETE FROM source_selector
WHERE concept_id = 'bokheim:subject:religion:movements'
  AND system_id = 'bisac'
  AND selector = 'REL068000';

INSERT INTO source_selector (concept_id, system_id, selector, ordinal)
VALUES
  ('bokheim:religion:theosophy', 'bisac', 'REL068000', 0),
  ('bokheim:religion:theosophy', 'lcc', 'BP500..BP585@1', 0),
  ('bokheim:religion:anthroposophy', 'lcc', 'BP595..BP597@1', 0),
  ('bokheim:religion:other-beliefs-movements', 'lcc', 'BP600..BP610@1', 0);

COMMIT;
