-- Remove World History's enumerated blocks while reusing existing era concepts.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Reuse existing leaf concepts in the World History browse path.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,3701,COALESCE((SELECT max(p.ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM concept c WHERE c.concept_id IN (3620,3608,3666);

CREATE TEMP TABLE _wh_base(id INTEGER NOT NULL);
INSERT INTO _wh_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _wh_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _wh_node VALUES
 (1,'General History Reference','D1..D11.5'),
 (2,'Historiography','D13..D15'),
 (3,'History Study & Teaching','D16.2..D16.5'),
 (4,'Collective Historical Biography','D34.2..D34.22'),
 (5,'Medieval & Modern History—General','D100.2..D110.5'),
 (6,'Eastern Hemisphere—General','D890..D893');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _wh_node n CROSS JOIN _wh_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3701,0 FROM _wh_node n CROSS JOIN _wh_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _wh_node n CROSS JOIN _wh_base b;

-- Everything represented by the new leaves or reused branches leaves the parent.
DELETE FROM source_selector WHERE concept_id=3701 AND system_id='lcc' AND (
 selector IN ('D1','D2','D2.5','D3.A2','D3.A22','D5','D5.5','D6','D7','D8','D9','D10','D11','D11.5','D13..D15',
              'D16.2','D16.25','D16.255.A8','D16.255.C65','D16.255.S5','D16.3','D27','D34.2','D890..D893')
 OR selector GLOB 'D10[0-9]*' OR selector GLOB 'D11[0]*'
);

DROP TABLE _wh_node;
DROP TABLE _wh_base;
COMMIT;
