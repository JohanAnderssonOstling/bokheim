-- Give Italy's DG schedule to mutually exclusive terminal chronology/region
-- nodes. KRY is an African law schedule and is deliberately discarded here.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=13624 AND system_id='lcc';
INSERT INTO source_selector VALUES(13624,'lcc','DG51..DG70.Z');

DELETE FROM source_selector WHERE concept_id=3575 AND system_id='lcc';
INSERT INTO source_selector VALUES(3575,'lcc','DG500..DG537');

DELETE FROM source_selector WHERE concept_id=8279 AND system_id='lcc';
INSERT INTO source_selector VALUES(8279,'lcc','DG581..DG590');

INSERT OR IGNORE INTO source_selector VALUES(2464,'lcc','DG421.5..DG430.2');

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (10310,10312,10330,10369,10416,10464,10498,10514,10537,10562);
INSERT INTO source_selector VALUES
 (10310,'lcc','DG819..DG831'),
 (10312,'lcc','DG651..DG664.5'),
 (10330,'lcc','DG610..DG618.78.Z'),
 (10369,'lcc','DG731..DG759.3'),
 (10416,'lcc','DG670..DG684.72'),
 (10464,'lcc','DG861..DG875'),
 (10498,'lcc','DG840..DG857.52'),
 (10514,'lcc','DG631..DG645'),
 (10537,'lcc','DG803..DG817.32'),
 (10562,'lcc','DG791..DG800');

CREATE TEMP TABLE _italy_base(id INTEGER NOT NULL);
INSERT INTO _italy_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _italy_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _italy_node VALUES
 (1,'Medieval & Modern Italy — General & Culture','DG401..DG421','DG431..DG499'),
 (2,'Northern Italy','DG600..DG609',NULL),
 (3,'Central Italy','DG691..DG694',NULL),
 (4,'Islands & Former Colonies','DG972..DG975.Z','DG980..DG980.22');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _italy_node n CROSS JOIN _italy_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3607,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3607),0)+n.seq-1
FROM _italy_node n CROSS JOIN _italy_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _italy_node n CROSS JOIN _italy_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _italy_node n CROSS JOIN _italy_base b
WHERE n.selector_b IS NOT NULL;

DELETE FROM source_selector WHERE concept_id=3607 AND system_id='lcc';

DROP TABLE _italy_node;
DROP TABLE _italy_base;
COMMIT;
