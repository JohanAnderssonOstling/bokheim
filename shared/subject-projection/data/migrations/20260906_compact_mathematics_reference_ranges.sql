-- Compact Mathematics history/reference enumerations into official leaf spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=4947 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (4947,'lcc','QA8..QA10.4'),
 (4947,'lcc','QA21..QA27');

CREATE TEMP TABLE _mr_base(id INTEGER NOT NULL);
INSERT INTO _mr_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _mr_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _mr_node VALUES
 (1,'Mathematicians & Biography','QA27.5..QA29'),
 (2,'Early Mathematical Works','QA30.3..QA35'),
 (3,'General Mathematical Works','QA36..QA39.22'),
 (4,'Mathematical Communication','QA41.4..QA42');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _mr_node n CROSS JOIN _mr_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,5722,0 FROM _mr_node n CROSS JOIN _mr_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _mr_node n CROSS JOIN _mr_base b;

DELETE FROM source_selector WHERE concept_id=5722 AND system_id='lcc' AND (
 selector='QA1-939' OR selector GLOB 'QA27.5*' OR selector GLOB 'QA28*'
 OR selector GLOB 'QA29*' OR selector GLOB 'QA30.3*' OR selector IN ('QA31','QA32','QA33','QA35')
 OR selector GLOB 'QA36*' OR selector GLOB 'QA37*' OR selector GLOB 'QA39*'
 OR selector GLOB 'QA41.4*' OR selector GLOB 'QA41.5*' OR selector GLOB 'QA41.6*'
 OR selector GLOB 'QA41.7*' OR selector='QA42'
);

DROP TABLE _mr_node;
DROP TABLE _mr_base;
COMMIT;
