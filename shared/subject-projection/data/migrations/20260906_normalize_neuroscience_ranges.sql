-- Remove exact enumerations already belonging to broad neurology/psychiatry
-- branches and range the remaining neuroscience foundations.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (2713,7901,9398,6666,2837,9435,8688,8882);
INSERT INTO source_selector VALUES
 (8688,'lcc','RC331'),
 (9435,'lcc','RC337..RC337.9'),
 (6666,'lcc','RC338..RC339'),
 (9398,'lcc','RC339.5..RC339.52'),
 (8882,'lcc','RC343.3'),
 (7901,'lcc','RC346..RC429.2'),
 (2837,'lcc','RC434.2..RC574');

CREATE TEMP TABLE _neuro_base(id INTEGER NOT NULL);
INSERT INTO _neuro_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'General & Clinical Foundations' FROM _neuro_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,2713,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=2713),0)
FROM _neuro_base;
INSERT INTO source_selector SELECT id+1,'lcc','RC321..RC330.Z' FROM _neuro_base;
INSERT INTO source_selector SELECT id+1,'lcc','RC332..RC336.Z' FROM _neuro_base;
INSERT INTO source_selector SELECT id+1,'lcc','RC340..RC343.29' FROM _neuro_base;
INSERT INTO source_selector SELECT id+1,'lcc','RC343.4..RC345.Z' FROM _neuro_base;
DROP TABLE _neuro_base;

COMMIT;
