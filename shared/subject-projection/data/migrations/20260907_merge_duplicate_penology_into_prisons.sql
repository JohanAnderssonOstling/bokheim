BEGIN;

DELETE FROM lcc_range WHERE concept_id=9656;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5465,'HV8301-9920.7');

CREATE TEMP TABLE _penology(label TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _penology VALUES
 ('U.S. Penology Records','HV8311','HV8480'),
 ('Correctional Statistics','HV8482','HV8488'),
 ('History of Punishment','HV8497','HV8544.22'),
 ('Early Forms of Punishment','HV8545','HV8654'),
 ('Famous Prisons & Prisoners','HV8655','HV8658'),
 ('Penological Theory','HV8661','HV8692.2'),
 ('Modern Forms of Punishment','HV8693','HV8749'),
 ('Prison Methods & Practice','HV8750.2','HV8931'),
 ('Prison Architecture','HV8805','HV8829'),
 ('Prison Health & Nutrition','HV8833','HV8849'),
 ('Prisoner Education & Support','HV8865','HV8886'),
 ('Prison Labor','HV8888','HV8931'),
 ('Penal Colonies','HV8935','HV8962'),
 ('Concentration & Internment Camps','HV8963','HV8964'),
 ('Prison Reform','HV8970.2','HV9018');

INSERT INTO concept(preferred_label) SELECT label FROM _penology p
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=p.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,5465,0 FROM _penology p JOIN concept c ON c.preferred_label=p.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT p.start_code,p.end_code,c.concept_id FROM _penology p JOIN concept c ON c.preferred_label=p.label;
DROP TABLE _penology;

-- Preserve both original navigation routes with the canonical subject.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 5465,5472,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent
WHERE concept_id=5465 AND NOT EXISTS(
 SELECT 1 FROM concept_parent WHERE concept_id=5465 AND parent_concept_id=5472);

DELETE FROM source_label WHERE concept_id=9656;
DELETE FROM lcc_selector WHERE concept_id=9656;
DELETE FROM bisac_selector WHERE concept_id=9656;
DELETE FROM concept_parent WHERE concept_id=9656 OR parent_concept_id=9656;
DELETE FROM concept WHERE concept_id=9656;

COMMIT;
