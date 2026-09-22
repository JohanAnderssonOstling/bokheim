-- Compact Netherlands into official general, chronological and local-history
-- ranges. Detailed wars and stadtholders remain absorbed by their broad eras.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN (11530,11531,11532,11533);
INSERT INTO source_selector VALUES
 (11533,'lcc','DJ95..DJ152'),
 (11530,'lcc','DJ154..DJ210'),
 (11532,'lcc','DJ211'),
 (11531,'lcc','DJ215..DJ295.Z');

CREATE TEMP TABLE _nl_base(id INTEGER NOT NULL);
INSERT INTO _nl_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _nl_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _nl_node VALUES
 (1,'General & Reference','DJ1..DJ92'),
 (2,'Local History & Description','DJ401..DJ500.22');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _nl_node n CROSS JOIN _nl_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3579,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3579),0)+n.seq-1
FROM _nl_node n CROSS JOIN _nl_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _nl_node n CROSS JOIN _nl_base b;

DELETE FROM source_selector WHERE concept_id=3579 AND system_id='lcc';
INSERT INTO source_selector VALUES(3579,'lcc','DJ');

DROP TABLE _nl_node;
DROP TABLE _nl_base;
COMMIT;
