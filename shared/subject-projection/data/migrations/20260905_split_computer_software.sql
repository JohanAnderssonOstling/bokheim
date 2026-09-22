-- Keep general software publications on Computer Software. Route identifiable
-- software topics to children and the observed Java alias to the existing Java
-- subject. Every selector here is an exact code, including literal range strings.
-- QA76.76.A65 and QA76.76.S46: local LCC MDSConnect 2016 captions.
-- QA76.76.J38 is used for Java in these catalogue records (not an official
-- caption in the local schedule):
-- https://openlibrary.org/books/OL25982279M/The_Well-grounded_Java_Developer
-- https://openlibrary.org/works/OL5970265W/Data_structures_and_algorithms_in_Java

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES
 ('bokheim:computing:application-software', 'Application Software'),
 ('bokheim:computing:shareware', 'Shareware');

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
 ('bokheim:computing:application-software', 'bokheim:computing-specific:0cd019c5d8d36ebf1b63', 0, 10),
 ('bokheim:computing:shareware', 'bokheim:computing-specific:0cd019c5d8d36ebf1b63', 0, 20);

UPDATE source_selector
SET concept_id = 'bokheim:computing:application-software', ordinal = 0
WHERE concept_id = 'bokheim:computing-specific:0cd019c5d8d36ebf1b63'
  AND system_id = 'lcc' AND selector = 'QA76.76.A65';

UPDATE source_selector
SET concept_id = 'bokheim:computing:shareware', ordinal = 0
WHERE concept_id = 'bokheim:computing-specific:0cd019c5d8d36ebf1b63'
  AND system_id = 'lcc' AND selector = 'QA76.76.S46';

UPDATE source_selector
SET concept_id = 'unified:d38119fb-8b87-5d84-88e1-8ab64a6e897e',
    ordinal = (SELECT COALESCE(MAX(ordinal) + 1, 0)
               FROM source_selector
               WHERE concept_id = 'unified:d38119fb-8b87-5d84-88e1-8ab64a6e897e'
                 AND system_id = 'lcc')
WHERE concept_id = 'bokheim:computing-specific:0cd019c5d8d36ebf1b63'
  AND system_id = 'lcc' AND selector = 'QA76.76.J38';

COMMIT;
