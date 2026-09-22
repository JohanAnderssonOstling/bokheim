-- Split Marketing using its BISAC subdivisions. The pruned broad LCC range
-- belongs to the General leaf, never to the now non-leaf parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id, preferred_label) VALUES
  ('bokheim:marketing:general', 'General Marketing'),
  ('bokheim:marketing:direct', 'Direct Marketing'),
  ('bokheim:marketing:industrial', 'Industrial Marketing'),
  ('bokheim:marketing:multilevel', 'Multilevel Marketing'),
  ('bokheim:marketing:research', 'Marketing Research'),
  ('bokheim:marketing:telemarketing', 'Telemarketing');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:marketing:general', 'unified:54299b57-991f-5cd6-9dbd-192916fefc07', 0, 100),
  ('bokheim:marketing:direct', 'unified:54299b57-991f-5cd6-9dbd-192916fefc07', 0, 200),
  ('bokheim:marketing:industrial', 'unified:54299b57-991f-5cd6-9dbd-192916fefc07', 0, 300),
  ('bokheim:marketing:multilevel', 'unified:54299b57-991f-5cd6-9dbd-192916fefc07', 0, 400),
  ('bokheim:marketing:research', 'unified:54299b57-991f-5cd6-9dbd-192916fefc07', 0, 500),
  ('bokheim:marketing:telemarketing', 'unified:54299b57-991f-5cd6-9dbd-192916fefc07', 0, 600);

UPDATE source_selector SET concept_id='bokheim:marketing:general', ordinal=0
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='bisac' AND selector='BUS043000';
UPDATE source_selector SET concept_id='bokheim:marketing:general', ordinal=0
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='lcc' AND selector='HF5410..HF5417.5@1';
UPDATE source_selector SET concept_id='bokheim:marketing:general', ordinal=1
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='lcc' AND selector='HF5410-5417.5';
UPDATE source_selector SET concept_id='bokheim:marketing:direct', ordinal=0
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='bisac' AND selector='BUS043010';
UPDATE source_selector SET concept_id='bokheim:marketing:industrial', ordinal=0
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='bisac' AND selector='BUS043020';
UPDATE source_selector SET concept_id='bokheim:marketing:multilevel', ordinal=0
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='bisac' AND selector='BUS043040';
UPDATE source_selector SET concept_id='bokheim:marketing:research', ordinal=0
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='bisac' AND selector='BUS043060';
UPDATE source_selector SET concept_id='bokheim:marketing:telemarketing', ordinal=0
 WHERE concept_id='unified:54299b57-991f-5cd6-9dbd-192916fefc07' AND system_id='bisac' AND selector='BUS043050';

COMMIT;
