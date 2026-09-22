-- Complete the BP Islam partition with residual leaves. Bahai, Theosophy,
-- Anthroposophy and other movements already own their official spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _bp_base(id INTEGER NOT NULL);
INSERT INTO _bp_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _bp_node(seq INTEGER PRIMARY KEY,label TEXT,selector1 TEXT,selector2 TEXT);
INSERT INTO _bp_node VALUES
 (1,'Islam—General & Reference','BP1..BP41.9',NULL),
 (2,'General Works on Islam','BP160..BP161.2',NULL),
 (3,'Islam in Relation to Special Topics','BP190.5.A..BP190.5.Z',NULL),
 (4,'Other Islamic Branches & Sects','BP191..BP191.9','BP194.93..BP253');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _bp_node n CROSS JOIN _bp_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4605,n.seq FROM _bp_node n CROSS JOIN _bp_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector1 FROM _bp_node n CROSS JOIN _bp_base b
UNION ALL
SELECT b.id+n.seq,'lcc',n.selector2 FROM _bp_node n CROSS JOIN _bp_base b
WHERE n.selector2 IS NOT NULL;

-- Normalize the already-pruned movement leaves.
DELETE FROM source_selector WHERE concept_id=4550 AND system_id='lcc';
INSERT INTO source_selector VALUES(4550,'lcc','BP300..BP420.5');
DELETE FROM source_selector WHERE concept_id=12242 AND system_id='lcc';
INSERT INTO source_selector VALUES(12242,'lcc','BP500..BP585');
DELETE FROM source_selector WHERE concept_id=12243 AND system_id='lcc';
INSERT INTO source_selector VALUES(12243,'lcc','BP595..BP597');
DELETE FROM source_selector WHERE concept_id=12244 AND system_id='lcc';
INSERT INTO source_selector VALUES(12244,'lcc','BP600..BP610');

DELETE FROM source_selector WHERE concept_id=5859 AND system_id='lcc';

DROP TABLE _bp_node;
DROP TABLE _bp_base;
COMMIT;
