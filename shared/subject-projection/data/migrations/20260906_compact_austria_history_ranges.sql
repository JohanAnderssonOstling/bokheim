-- Compact Austria into disjoint general, era and regional LC spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=3536 AND system_id='lcc';
INSERT INTO source_selector VALUES(3536,'lcc','DB65..DB74.91');
DELETE FROM source_selector WHERE concept_id=10214 AND system_id='lcc';
INSERT INTO source_selector VALUES(10214,'lcc','DB51..DB59');
DELETE FROM source_selector WHERE concept_id=11558 AND system_id='lcc';
INSERT INTO source_selector VALUES(11558,'lcc','DB77.3..DB82.Z');
INSERT INTO source_selector VALUES(11558,'lcc','DB84..DB98.9');

CREATE TEMP TABLE _austria_base(id INTEGER NOT NULL);
INSERT INTO _austria_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _austria_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT,selector_c TEXT,selector_d TEXT,selector_e TEXT,selector_f TEXT);
INSERT INTO _austria_node VALUES
 (1,'General & Reference','DB1..DB49',NULL,NULL,NULL,NULL,NULL),
 (2,'Alps','DB101..DB110.15',NULL,NULL,NULL,NULL,NULL),
 (3,'Carinthia','DB281..DB300',NULL,NULL,NULL,NULL,NULL),
 (4,'Danube River Region','DB441..DB450.7',NULL,NULL,NULL,NULL,NULL),
 (5,'Salzburg','DB601..DB620',NULL,NULL,NULL,NULL,NULL),
 (6,'Salzkammergut','DB621..DB640',NULL,NULL,NULL,NULL,NULL),
 (7,'Styria','DB681..DB700',NULL,NULL,NULL,NULL,NULL),
 (8,'Tyrol & Vorarlberg','DB761..DB780',NULL,NULL,NULL,NULL,NULL),
 (9,'Vienna','DB841..DB860',NULL,NULL,NULL,NULL,NULL),
 (10,'Other Austrian Regions','DB110.16..DB280.Z','DB301..DB440.Z','DB451..DB600.Z','DB641..DB680.Z','DB701..DB760.Z','DB781..DB785');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _austria_node n CROSS JOIN _austria_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3537,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3537),0)+n.seq-1
FROM _austria_node n CROSS JOIN _austria_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',v.selector
FROM _austria_node n CROSS JOIN _austria_base b
JOIN (
 SELECT seq,selector_a selector FROM _austria_node UNION ALL
 SELECT seq,selector_b FROM _austria_node WHERE selector_b IS NOT NULL UNION ALL
 SELECT seq,selector_c FROM _austria_node WHERE selector_c IS NOT NULL UNION ALL
 SELECT seq,selector_d FROM _austria_node WHERE selector_d IS NOT NULL UNION ALL
 SELECT seq,selector_e FROM _austria_node WHERE selector_e IS NOT NULL UNION ALL
 SELECT seq,selector_f FROM _austria_node WHERE selector_f IS NOT NULL
) v ON v.seq=n.seq;

DELETE FROM source_selector WHERE concept_id=3537 AND system_id='lcc';

DROP TABLE _austria_node;
DROP TABLE _austria_base;
COMMIT;
