PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Fascism & Totalitarianism — General'
WHERE concept_id='bokheim:politics:ideology:fascism-totalitarianism';

DELETE FROM source_selector
WHERE concept_id='bokheim:politics:ideology:fascism-totalitarianism'
  AND system_id='lcc';

INSERT INTO concept(concept_id,preferred_label) VALUES
('bokheim:politics:ideology:fascism','Fascism'),
('bokheim:politics:ideology:totalitarianism','Totalitarianism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
('bokheim:politics:ideology:fascism','bokheim:politics:theory-ideologies',0,71),
('bokheim:politics:ideology:totalitarianism','bokheim:politics:theory-ideologies',0,72);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:politics:ideology:totalitarianism','lcc','JC480..JC480.99@5',0),
('bokheim:politics:ideology:fascism','lcc','JC481..JC481.99@5',0);

COMMIT;
