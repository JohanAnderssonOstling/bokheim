BEGIN;

DELETE FROM lcc_range WHERE concept_id=5465 AND class_letters='HV';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5465,'HV9051-9230.7'),
 (5465,'HV9261-9430.7'),
 (5465,'HV9441-9920.7');

WITH wanted(label) AS (VALUES
 ('Juvenile Offenders & Reform Schools'),
 ('Prisoner Rehabilitation'),
 ('Alternatives to Imprisonment'),
 ('International Prison Systems'),
 ('U.S. Prisons'),
 ('Prison Administration'),
 ('Prisons by Country'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5465,0 FROM concept WHERE preferred_label IN (
 'Juvenile Offenders & Reform Schools','Prisoner Rehabilitation',
 'Alternatives to Imprisonment','International Prison Systems','U.S. Prisons',
 'Prison Administration','Prisons by Country');

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','9051','9230.7',concept_id FROM concept
WHERE preferred_label='Juvenile Offenders & Reform Schools';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','9261','9430.7',concept_id FROM concept
WHERE preferred_label='Prisoner Rehabilitation';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','9276.5','9279',concept_id FROM concept
WHERE preferred_label='Alternatives to Imprisonment';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','9441','9443',concept_id FROM concept
WHERE preferred_label='International Prison Systems';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','9455.2','9481',concept_id FROM concept
WHERE preferred_label='U.S. Prisons';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','9469','9470',concept_id FROM concept
WHERE preferred_label='Prison Administration';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'HV','9501','9920.7',concept_id FROM concept
WHERE preferred_label='Prisons by Country';

COMMIT;
