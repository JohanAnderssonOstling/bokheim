-- Complete the Algebra schedule with official ranged leaves and remove the
-- parent enumeration/legacy hyphen umbrellas.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='General Algebra & Textbooks'
WHERE concept_id=4919;
DELETE FROM source_selector WHERE concept_id=4919 AND system_id='lcc';
INSERT INTO source_selector VALUES(4919,'lcc','QA150..QA161.Z');
INSERT OR IGNORE INTO source_selector VALUES(4946,'lcc','QA171');

CREATE TEMP TABLE _alg_base(id INTEGER NOT NULL);
INSERT INTO _alg_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _alg_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _alg_node VALUES
 (1,'Theory of Equations','QA211..QA218'),
 (2,'Abstract Machines & Automata','QA267..QA268.5');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _alg_node n CROSS JOIN _alg_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4917,n.seq FROM _alg_node n CROSS JOIN _alg_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _alg_node n CROSS JOIN _alg_base b;

DELETE FROM source_selector WHERE concept_id=4917 AND system_id='lcc';

DROP TABLE _alg_node;
DROP TABLE _alg_base;
COMMIT;
