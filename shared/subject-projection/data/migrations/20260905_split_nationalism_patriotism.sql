PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- BISAC POL031000 combines these topics; retain it on an explicit general leaf.
UPDATE concept SET preferred_label='Nationalism & Patriotism — General'
WHERE concept_id='bokheim:politics:ideology:nationalism-patriotism';

-- Move the distinct LCC schedules onto mutually exclusive sibling leaves.
DELETE FROM source_selector
WHERE concept_id='bokheim:politics:ideology:nationalism-patriotism'
  AND system_id='lcc';

INSERT INTO concept(concept_id,preferred_label) VALUES
('bokheim:politics:ideology:nationalism','Nationalism'),
('bokheim:politics:ideology:patriotism','Patriotism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
('bokheim:politics:ideology:nationalism','bokheim:politics:theory-ideologies',0,61),
('bokheim:politics:ideology:patriotism','bokheim:politics:theory-ideologies',0,62);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:politics:ideology:nationalism','lcc','JC311..JC314@5',0),
('bokheim:politics:ideology:patriotism','lcc','JC329..JC329.99@5',0);

COMMIT;
