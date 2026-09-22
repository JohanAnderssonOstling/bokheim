-- Replace enumerated English county and town Cutters with two schedule-backed leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _england_range_base(id INTEGER NOT NULL);
INSERT INTO _england_range_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Counties & Regions' FROM _england_range_base
WHERE EXISTS(SELECT 1 FROM source_selector WHERE concept_id=3566 AND system_id='lcc' AND selector LIKE 'DA670.%');
INSERT INTO concept(concept_id,preferred_label)
SELECT id+2,'Cities & Towns' FROM _england_range_base
WHERE EXISTS(SELECT 1 FROM source_selector WHERE concept_id=3566 AND system_id='lcc' AND selector LIKE 'DA690.%');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,3566,0 FROM _england_range_base WHERE EXISTS(SELECT 1 FROM concept WHERE concept_id=id+1);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+2,3566,0 FROM _england_range_base WHERE EXISTS(SELECT 1 FROM concept WHERE concept_id=id+2);

INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','DA670.A..DA670.Z' FROM _england_range_base WHERE EXISTS(SELECT 1 FROM concept WHERE concept_id=id+1);
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','DA690.A..DA690.Z' FROM _england_range_base WHERE EXISTS(SELECT 1 FROM concept WHERE concept_id=id+2);

DELETE FROM source_selector
WHERE concept_id=3566 AND system_id='lcc'
  AND (selector LIKE 'DA670.%' OR selector LIKE 'DA690.%');

DROP TABLE _england_range_base;
COMMIT;
