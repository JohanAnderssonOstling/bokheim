PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Social History & Conditions — General'
WHERE concept_id='unified:0f75efff-41d0-507d-95b7-6e30c4273d0b';

-- Preserve HN as a deliberately broad fallback leaf without imported overlap.
DELETE FROM source_selector
WHERE concept_id='unified:0f75efff-41d0-507d-95b7-6e30c4273d0b'
  AND system_id='lcc';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:0f75efff-41d0-507d-95b7-6e30c4273d0b','lcc','HN',0),
('unified:0f75efff-41d0-507d-95b7-6e30c4273d0b','lcc','HN1..HN995@1',1);

CREATE TEMP TABLE social_split(
 concept_id TEXT PRIMARY KEY,
 label TEXT NOT NULL,
 bisac TEXT NOT NULL UNIQUE,
 display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO social_split VALUES
('bokheim:social-problems:disability','People with Disabilities','SOC029000',200),
('bokheim:social-problems:discrimination','Discrimination','SOC031000',210),
('bokheim:social-problems:violence','Violence in Society','SOC051000',220),
('bokheim:social-problems:slavery','Slavery','SOC054000',230),
('bokheim:social-problems:disease-health','Disease & Health Issues','SOC057000',240),
('bokheim:social-problems:sex-trade','Prostitution & Sex Trade','SOC059000',250),
('bokheim:social-problems:sexual-abuse-harassment','Sexual Abuse & Harassment','SOC060000',260),
('bokheim:social-problems:capital-punishment','Capital Punishment','SOC067000',280),
('bokheim:social-problems:human-trafficking','Human Trafficking','SOC073000',290);

DELETE FROM source_selector
WHERE concept_id='unified:0f75efff-41d0-507d-95b7-6e30c4273d0b'
  AND system_id='bisac';

INSERT INTO concept(concept_id,preferred_label)
SELECT concept_id,label FROM social_split;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,'bokheim:sociology:inequality-problems',0,display_order
FROM social_split;
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'bisac',bisac,0 FROM social_split;

-- Reuse the existing general Refugee Problems concept in both relevant branches.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES('unified:548e69f6-a62a-5c7a-9b27-787ace07eed6','bisac','SOC066000',0);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
VALUES('unified:548e69f6-a62a-5c7a-9b27-787ace07eed6','bokheim:sociology:inequality-problems',0,270);

COMMIT;
