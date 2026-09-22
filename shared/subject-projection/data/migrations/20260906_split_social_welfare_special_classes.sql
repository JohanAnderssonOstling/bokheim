-- Split the overloaded LCC "Special classes" welfare leaf along the seven
-- mutually exclusive groups represented by its current selectors.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id, preferred_label) VALUES
  ('bokheim:social-welfare:special-classes:families-mothers', 'Families & Mothers'),
  ('bokheim:social-welfare:special-classes:children', 'Children'),
  ('bokheim:social-welfare:special-classes:young-adults', 'Young Adults & Teenagers'),
  ('bokheim:social-welfare:special-classes:women', 'Women'),
  ('bokheim:social-welfare:special-classes:lgbtq', 'LGBTQ+ People'),
  ('bokheim:social-welfare:special-classes:older-people', 'Older People'),
  ('bokheim:social-welfare:special-classes:disabilities', 'People with Disabilities');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:social-welfare:special-classes:families-mothers', 'unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f', 0, 100),
  ('bokheim:social-welfare:special-classes:children', 'unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f', 0, 200),
  ('bokheim:social-welfare:special-classes:young-adults', 'unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f', 0, 300),
  ('bokheim:social-welfare:special-classes:women', 'unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f', 0, 400),
  ('bokheim:social-welfare:special-classes:lgbtq', 'unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f', 0, 500),
  ('bokheim:social-welfare:special-classes:older-people', 'unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f', 0, 600),
  ('bokheim:social-welfare:special-classes:disabilities', 'unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f', 0, 700);

UPDATE source_selector SET concept_id='bokheim:social-welfare:special-classes:families-mothers', ordinal=0
 WHERE concept_id='unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f' AND system_id='lcc' AND selector='HV697..HV700.7@4';
UPDATE source_selector SET concept_id='bokheim:social-welfare:special-classes:children', ordinal=0
 WHERE concept_id='unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f' AND system_id='lcc' AND selector='HV701..HV1420.5@4';
UPDATE source_selector SET concept_id='bokheim:social-welfare:special-classes:young-adults', ordinal=0
 WHERE concept_id='unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f' AND system_id='lcc' AND selector='HV1421..HV1441@4';
UPDATE source_selector SET concept_id='bokheim:social-welfare:special-classes:women', ordinal=0
 WHERE concept_id='unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f' AND system_id='lcc' AND selector='HV1442..HV1448@4';
UPDATE source_selector SET concept_id='bokheim:social-welfare:special-classes:lgbtq', ordinal=0
 WHERE concept_id='unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f' AND system_id='lcc' AND selector='HV1449';
UPDATE source_selector SET concept_id='bokheim:social-welfare:special-classes:older-people', ordinal=0
 WHERE concept_id='unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f' AND system_id='lcc' AND selector='HV1450..HV1494@4';
UPDATE source_selector SET concept_id='bokheim:social-welfare:special-classes:disabilities', ordinal=0
 WHERE concept_id='unified:a3dc2a34-bb5a-54b8-9d89-5d3496321b4f' AND system_id='lcc' AND selector='HV1551..HV3024@4';

COMMIT;
