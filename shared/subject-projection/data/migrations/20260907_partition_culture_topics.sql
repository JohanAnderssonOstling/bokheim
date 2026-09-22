BEGIN;

DELETE FROM lcc_range
WHERE concept_id=12256 AND class_letters='HM'
  AND ((start_value='621' AND end_value='656') OR
       (start_value='106' AND end_value='106'));
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (12256,'HM621-HM656'),
 (12256,'HM621-656'),
 (12256,'HM106');

WITH wanted(label) AS (VALUES
 ('Cultural Studies'),
 ('Cultural Evolution'),
 ('Sociobiology'),
 ('Social Darwinism'),
 ('Body & Culture'),
 ('Ideology'),
 ('Subcultures & Counterculture'),
 ('Space & Society'),
 ('Time & Society'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,12256,0 FROM concept WHERE preferred_label IN (
 'Cultural Studies','Cultural Evolution','Sociobiology','Social Darwinism',
 'Body & Culture','Ideology','Subcultures & Counterculture',
 'Space & Society','Time & Society');

-- Reuse the existing Sociology of Knowledge concept in this polyhierarchy.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 10853,12256,COALESCE(MAX(ordinal),-1)+1
FROM concept_parent WHERE concept_id=10853
  AND NOT EXISTS (
    SELECT 1 FROM concept_parent
    WHERE concept_id=10853 AND parent_concept_id=12256
  );

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','623','623',concept_id FROM concept WHERE preferred_label='Cultural Studies';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','626','631',concept_id FROM concept WHERE preferred_label='Cultural Evolution';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','628','628',concept_id FROM concept WHERE preferred_label='Sociobiology';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','631','631',concept_id FROM concept WHERE preferred_label='Social Darwinism';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','636','636',concept_id FROM concept WHERE preferred_label='Body & Culture';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','641','641',concept_id FROM concept WHERE preferred_label='Ideology';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','646','647',concept_id FROM concept WHERE preferred_label='Subcultures & Counterculture';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('HM','651','651',10853);
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','654','654',concept_id FROM concept WHERE preferred_label='Space & Society';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HM','656','656',concept_id FROM concept WHERE preferred_label='Time & Society';

COMMIT;
