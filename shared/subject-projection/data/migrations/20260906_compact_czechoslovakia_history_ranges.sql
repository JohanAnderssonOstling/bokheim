-- Compact the Czechoslovak/Czech DB schedule into broad chronology and local
-- history leaves. The Revolution of 1848 remains absorbed by Habsburg rule.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN (10095,10124,10199);
INSERT INTO source_selector VALUES
 (10124,'lcc','DB2080..DB2133'),
 (10095,'lcc','DB2135..DB2182'),
 (10199,'lcc','DB2185..DB2241');

CREATE TEMP TABLE _cz_base(id INTEGER NOT NULL);
INSERT INTO _cz_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _cz_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _cz_node VALUES
 (1,'General & Reference','DB2000..DB2043.9'),
 (2,'General Historical Studies','DB2043.92..DB2079.Z'),
 (3,'Independent Czech Republic, 1993–Present','DB2242..DB2247'),
 (4,'Moravia','DB2300..DB2421'),
 (5,'Other Czech Lands','DB2422..DB2599.Z'),
 (6,'Prague','DB2600..DB2649');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _cz_node n CROSS JOIN _cz_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3538,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3538),0)+n.seq-1
FROM _cz_node n CROSS JOIN _cz_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _cz_node n CROSS JOIN _cz_base b;

DELETE FROM source_selector WHERE concept_id=3538 AND system_id='lcc';

DROP TABLE _cz_node;
DROP TABLE _cz_base;
COMMIT;
