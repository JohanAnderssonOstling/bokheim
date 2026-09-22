PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Remove the imported combined HX concept; its useful scope is represented by
-- the flat Communism/Socialism leaf, ending before Utopias and Anarchism.
DELETE FROM concept WHERE concept_id='unified:56d80652-27bf-591d-b99f-54405b056e80';

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:politics:ideology:communism-socialism','lcc','HX1..HX780.92@1',0),
('bokheim:politics:ideology:democracy','lcc','JC421..JC423@5',0),
('bokheim:politics:ideology:nationalism-patriotism','lcc','JC311..JC314@3',0),
('bokheim:politics:ideology:nationalism-patriotism','lcc','JC329',1),
('bokheim:politics:ideology:fascism-totalitarianism','lcc','JC480..JC481@5',0);

COMMIT;
