-- Normalize Denmark, Norway, Sweden, and Finland to their official broad DL
-- schedule blocks. Each range has one owner and country parents retain no umbrella.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Denmark.
DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3580,11507,11508,11509,11510,11511,11512,11513,11514,11515,13747,13748);
INSERT INTO source_selector VALUES
 (11514,'lcc','DL159.92..DL166.8'),
 (11507,'lcc','DL167..DL173.8'),
 (11508,'lcc','DL174..DL176.8'),
 (11509,'lcc','DL177..DL181.Z'),
 (11515,'lcc','DL182..DL183.9'),
 (11510,'lcc','DL184..DL192.8'),
 (11511,'lcc','DL193..DL199.8'),
 (11512,'lcc','DL201..DL249'),
 (11513,'lcc','DL250..DL263.3'),
 (13747,'lcc','DL271.A..DL271.Z'),
 (13748,'lcc','DL276..DL291.Z');
UPDATE concept SET preferred_label='Early & Viking Denmark, to 1042' WHERE concept_id=11514;

-- Norway.
DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3583,11538,11535,11534,11537,11536,13744,13745,13746);
INSERT INTO source_selector VALUES
 (13744,'lcc','DL460..DL468.5'),
 (11534,'lcc','DL469..DL479.Z'),
 (11535,'lcc','DL480..DL499.Z'),
 (11537,'lcc','DL500..DL502'),
 (11536,'lcc','DL503..DL526'),
 (11538,'lcc','DL527..DL537'),
 (13745,'lcc','DL576.A..DL576.Z'),
 (13746,'lcc','DL581..DL596.Z');

-- Sweden.
DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3584,11556,11555,10132,11557,10206,10644,10665,10138,13749,13750);
INSERT INTO source_selector VALUES
 (11555,'lcc','DL672..DL679'),
 (10665,'lcc','DL681..DL695.Z'),
 (10644,'lcc','DL696..DL700.9'),
 (10206,'lcc','DL701..DL719.9'),
 (10132,'lcc','DL721..DL743'),
 (11556,'lcc','DL747..DL805'),
 (10138,'lcc','DL807..DL859'),
 (11557,'lcc','DL860..DL879'),
 (13749,'lcc','DL971.A..DL971.Z'),
 (13750,'lcc','DL976..DL991.Z');

-- Finland.
DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3581,11516,11517,11518,11519);
INSERT INTO source_selector VALUES
 (11516,'lcc','DL1060..DL1060.5'),
 (11517,'lcc','DL1063..DL1063.9'),
 (11519,'lcc','DL1065..DL1065.8'),
 (11518,'lcc','DL1066..DL1141.6');

CREATE TEMP TABLE _nordic_base(id INTEGER NOT NULL);
INSERT INTO _nordic_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _nordic_node(
 seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector_a TEXT,selector_b TEXT
);
INSERT INTO _nordic_node VALUES
 (1,3580,'General & Reference','DL101..DL114.5',NULL),
 (2,3580,'Culture & Ethnography','DL121..DL142.Z',NULL),
 (3,3580,'General Historical Studies','DL143..DL159.5',NULL),
 (4,3583,'General & Reference','DL401..DL414.Z',NULL),
 (5,3583,'Culture & Ethnography','DL420..DL442.Z',NULL),
 (6,3583,'General Historical Studies','DL443..DL459.Z',NULL),
 (7,3584,'General & Reference','DL601..DL614.5',NULL),
 (8,3584,'Culture & Ethnography','DL621..DL641.Z',NULL),
 (9,3584,'General Historical Studies','DL643..DL659.Z',NULL),
 (10,3584,'Early & Viking Sweden, to 1134','DL660..DL671.Z',NULL),
 (11,3581,'General & Reference','DL1002..DL1014.7',NULL),
 (12,3581,'Culture & Ethnography','DL1016..DL1022',NULL),
 (13,3581,'General Historical Studies','DL1023.2..DL1048.Z',NULL),
 (14,3581,'Early Finland, to 1523','DL1050..DL1052.9',NULL),
 (15,3581,'Finland under Swedish Rule, 1523–1617','DL1055..DL1058.5',NULL),
 (16,3581,'Regions & Provinces','DL1170.A..DL1170.Z',NULL),
 (17,3581,'Helsinki','DL1175..DL1175.95',NULL),
 (18,3581,'Other Cities & Towns','DL1180.A..DL1180.Z',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _nordic_node n CROSS JOIN _nordic_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _nordic_node n CROSS JOIN _nordic_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _nordic_node n CROSS JOIN _nordic_base b;

DROP TABLE _nordic_node;
DROP TABLE _nordic_base;
COMMIT;
