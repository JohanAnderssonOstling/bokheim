PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- BISAC POL042020 combines the two traditions, so retain it on a general leaf.
UPDATE concept SET preferred_label='Conservatism & Liberalism — General'
WHERE concept_id='bokheim:politics:ideology:conservatism-liberalism';

INSERT INTO concept(concept_id,preferred_label) VALUES
('bokheim:politics:ideology:conservatism','Conservatism'),
('bokheim:politics:ideology:liberalism','Liberalism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
('bokheim:politics:ideology:conservatism','bokheim:politics:theory-ideologies',0,51),
('bokheim:politics:ideology:liberalism','bokheim:politics:theory-ideologies',0,52);

-- Official LCC Schedule J: JC573 is conservatism; JC574 is liberalism.
-- These specific leaf ranges override the broad political-theory range.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:politics:ideology:conservatism','lcc','JC573..JC573.99@5',0),
('bokheim:politics:ideology:liberalism','lcc','JC574..JC574.99@5',0);

COMMIT;
