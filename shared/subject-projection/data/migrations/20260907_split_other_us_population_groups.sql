BEGIN;

WITH wanted(label) AS (VALUES
 ('Afghan American History'),
 ('Belarusian American History'),
 ('Brazilian American History'),
 ('Canadian American History'),
 ('Central American History'),
 ('Colombian American History'),
 ('Croatian American History'),
 ('Latvian American History'),
 ('Melungeon & Ramapo History'),
 ('Mormon American History'),
 ('Quaker American History'),
 ('Slovenian American History'),
 ('Southeast Asian American History'),
 ('Taiwanese American History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13702,0 FROM concept WHERE preferred_label IN (
 'Afghan American History','Belarusian American History','Brazilian American History',
 'Canadian American History','Central American History','Colombian American History',
 'Croatian American History','Latvian American History','Melungeon & Ramapo History',
 'Mormon American History','Quaker American History','Slovenian American History',
 'Southeast Asian American History','Taiwanese American History');

-- Established Jewish and African American subdivisions belong to their
-- existing history subjects, not to the population umbrella.
UPDATE lcc_selector SET concept_id=50876
WHERE concept_id=13702 AND (
 selector GLOB 'E184.35*' OR selector GLOB 'E184.36*');
UPDATE lcc_selector SET concept_id=3378
WHERE concept_id=13702 AND selector IN ('E184.5','E184.6','E184.65');

-- Reuse existing national, regional, religious, and cultural leaves where the
-- LCC caption is unambiguous.
UPDATE lcc_selector SET concept_id=50875
WHERE concept_id=13702 AND selector IN ('E184.B15','E184.S7');
UPDATE lcc_selector SET concept_id=50879
WHERE concept_id=13702 AND selector IN ('E184.P3','E184.R85');
UPDATE lcc_selector SET concept_id=51266
WHERE concept_id=13702 AND selector IN ('E184.C27','E184.J27');
UPDATE lcc_selector SET concept_id=51244
WHERE concept_id=13702 AND selector IN
 ('E184.P28','E184.B13','E184.B26','E184.G84','E184.S55');
UPDATE lcc_selector SET concept_id=51264
WHERE concept_id=13702 AND selector IN
 ('E184.E74','E184.B89','E184.C24','E184.L53','E184.S77');
UPDATE lcc_selector SET concept_id=51159
WHERE concept_id=13702 AND selector IN ('E184.L34','E184.S98','E184.E38');
UPDATE lcc_selector SET concept_id=51245
WHERE concept_id=13702 AND selector IN
 ('E184.A2','E184.A4','E184.F85','E184.H9');
UPDATE lcc_selector SET concept_id=51252
WHERE concept_id=13702 AND selector IN
 ('E184.A9','E184.B8','E184.E17','E184.E7','E184.G27','E184.M3','E184.Y7');
UPDATE lcc_selector SET concept_id=51242
WHERE concept_id=13702 AND selector IN ('E184.F57','E184.F894');
UPDATE lcc_selector SET concept_id=50874
WHERE concept_id=13702 AND selector='E184.M47';
UPDATE lcc_selector SET concept_id=50882
WHERE concept_id=13702 AND selector='E184.T53';
UPDATE lcc_selector SET concept_id=50876
WHERE concept_id=13702 AND selector='E184.I7';

-- Selector-bearing additions for populations that are not represented by an
-- existing leaf. Small closely related populations share a meaningful leaf.
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Afghan American History')
WHERE concept_id=13702 AND selector='E184.A23';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Belarusian American History')
WHERE concept_id=13702 AND selector='E184.W6';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Brazilian American History')
WHERE concept_id=13702 AND selector='E184.B68';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Canadian American History')
WHERE concept_id=13702 AND selector='E184.C2';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Central American History')
WHERE concept_id=13702 AND selector IN ('E184.C34','E184.G82','E184.H66','E184.P35');
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Colombian American History')
WHERE concept_id=13702 AND selector='E184.C58';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Croatian American History')
WHERE concept_id=13702 AND selector='E184.C93';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Latvian American History')
WHERE concept_id=13702 AND selector='E184.L4';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Melungeon & Ramapo History')
WHERE concept_id=13702 AND selector IN ('E184.M44','E184.R3');
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Mormon American History')
WHERE concept_id=13702 AND selector='E184.M8';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Quaker American History')
WHERE concept_id=13702 AND selector='E184.F89';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Slovenian American History')
WHERE concept_id=13702 AND selector='E184.S65';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Southeast Asian American History')
WHERE concept_id=13702 AND selector IN ('E184.I43','E184.S695','E184.T4');
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Taiwanese American History')
WHERE concept_id=13702 AND selector='E184.T35';

-- The few small Latin American populations without a useful dedicated leaf
-- remain together under the established Spanish American subject.
UPDATE lcc_selector SET concept_id=50875
WHERE concept_id=13702 AND selector IN ('E184.C4','E184.E28');

COMMIT;
