-- Replace the remaining BF occult-science enumeration with the broad spans
-- defined by the LC schedule.  Keep BF1409.5 separate because the curated
-- tree already exposes Relation to science, and attach BF1626 to Magic.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _occult_base(id INTEGER NOT NULL);
INSERT INTO _occult_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _occult_node(
  seq INTEGER PRIMARY KEY,
  label TEXT NOT NULL,
  selector_a TEXT NOT NULL,
  selector_b TEXT
);
INSERT INTO _occult_node VALUES
 (1,'General & Reference','BF1403.2..BF1409','BF1410..BF1418'),
 (2,'Regional & Special Topics','BF1434..BF1442.Z',NULL),
 (3,'Miscellaneous Occult Studies','BF1995..BF1999',NULL);

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _occult_node n CROSS JOIN _occult_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3294,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3294),0)+n.seq-1
FROM _occult_node n CROSS JOIN _occult_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _occult_node n CROSS JOIN _occult_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _occult_node n CROSS JOIN _occult_base b
WHERE n.selector_b IS NOT NULL;

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
VALUES(3300,'lcc','BF1626');

DELETE FROM source_selector WHERE concept_id=3294 AND system_id='lcc';

-- These narrower selectors are contained by a range already owned by the
-- same subject and only make selector counts misleading.
DELETE FROM source_selector
WHERE system_id='lcc' AND (
 (concept_id=3295 AND selector IN ('BF1680..BF1708.8','BF1715..BF1729')) OR
 (concept_id=3296 AND selector IN ('BF1508','BF1520..BF1538')) OR
 (concept_id=3297 AND selector IN ('BF1850..BF1868','BF1876..BF1891')) OR
 (concept_id=3298 AND selector='BF1445..BF1468') OR
 (concept_id=3300 AND selector='BF1600..BF1618') OR
 (concept_id=3303 AND selector='BF1565..BF1566')
);

DROP TABLE _occult_node;
DROP TABLE _occult_base;
COMMIT;
