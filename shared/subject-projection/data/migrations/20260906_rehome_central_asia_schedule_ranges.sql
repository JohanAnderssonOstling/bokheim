-- Move Central Asian national schedules out of the Eastern Europe umbrella and
-- compact them under their actual country nodes.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id=3522 AND system_id='lcc'
  AND CAST(substr(selector,3) AS REAL) BETWEEN 901 AND 949.999;

-- Normalize existing country-era leaves.
DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (11338,11379,11383,11398,11392,11387,11380,11360,11369,
  11373,11363,11372,11365,11384,11391,11375);
INSERT INTO source_selector VALUES
 (11379,'lcc','DK908.85..DK908.856'),
 (11338,'lcc','DK908.86..DK908.8614'),
 (11383,'lcc','DK908.8618..DK908.863'),
 (11398,'lcc','DK908.864..DK908.866'),
 (11392,'lcc','DK908.867..DK908.868'),
 (11387,'lcc','DK918.84..DK918.848'),
 (11380,'lcc','DK918.85..DK918.87'),
 (11360,'lcc','DK918.871..DK918.873'),
 (11369,'lcc','DK918.875..DK918.877'),
 (11373,'lcc','DK928.85..DK928.853'),
 (11363,'lcc','DK928.86..DK928.863'),
 (11372,'lcc','DK928.865..DK928.867'),
 (11365,'lcc','DK938.865..DK938.867'),
 (11384,'lcc','DK948.85..DK948.853'),
 (11391,'lcc','DK948.86..DK948.863'),
 (11375,'lcc','DK948.865..DK948.867');

CREATE TEMP TABLE _central_asia_base(id INTEGER NOT NULL);
INSERT INTO _central_asia_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _central_asia_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _central_asia_node VALUES
 (1,3590,'General & Earlier History','DK901..DK908.849'),
 (2,3590,'Regions & Cities','DK909..DK909.5.Z'),
 (3,3591,'General & Earlier History','DK911..DK918.839'),
 (4,3591,'Regions & Cities','DK919..DK919.5.Z'),
 (5,3596,'General & Earlier History','DK921..DK928.849'),
 (6,3596,'Regions & Cities','DK929..DK929.5.Z'),
 (7,3597,'General & Earlier History','DK931..DK938.849'),
 (8,3597,'1917–1945','DK938.85..DK938.853'),
 (9,3597,'1945–1991','DK938.86..DK938.863'),
 (10,3597,'Regions & Cities','DK939..DK939.5.Z'),
 (11,3598,'General & Earlier History','DK941..DK948.849'),
 (12,3598,'Regions & Cities','DK949..DK949.5.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _central_asia_node n CROSS JOIN _central_asia_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _central_asia_node n CROSS JOIN _central_asia_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _central_asia_node n CROSS JOIN _central_asia_base b;

DROP TABLE _central_asia_node;
DROP TABLE _central_asia_base;
COMMIT;
