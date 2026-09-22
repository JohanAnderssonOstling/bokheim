-- Move official general-economic-works and relation-to-other-fields spans to
-- leaves; remove restored all-schedule umbrellas from Economic Theory.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _econ_base(id INTEGER NOT NULL);
INSERT INTO _econ_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'General Economic Works' FROM _econ_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,5593,0 FROM _econ_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','HB151..HB181' FROM _econ_base;

INSERT OR IGNORE INTO source_selector
VALUES(5097,'lcc','HB74.2..HB74.9.Z');

DELETE FROM source_selector WHERE concept_id=5593 AND system_id='lcc'
 AND (selector IN ('HB1-3840','HB1-846.8')
      OR (selector GLOB 'HB1*' AND CAST(substr(selector,3) AS REAL) BETWEEN 151 AND 181.999)
      OR selector GLOB 'HB74.*');

DROP TABLE _econ_base;
COMMIT;
