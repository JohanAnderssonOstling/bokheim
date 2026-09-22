-- Finish the Recreation split using official LCC spans.
-- Sports already owns GV561-GV1198.995; this moves the remaining large
-- enumerations into mutually exclusive leaf ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _recreation_base(id INTEGER NOT NULL);
INSERT INTO _recreation_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

CREATE TEMP TABLE _recreation_node(
  seq INTEGER PRIMARY KEY,
  label TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  selector TEXT NOT NULL
);
INSERT INTO _recreation_node VALUES
  (1,'Sports Sciences',62,'GV557..GV558'),
  (2,'Amusement Parks',63,'GV1851..GV1860'),
  (3,'Hunting Sports',64,'SK1..SK664');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label
FROM _recreation_node n CROSS JOIN _recreation_base b
WHERE EXISTS(SELECT 1 FROM concept WHERE concept_id=4628)
  AND NOT EXISTS(
    SELECT 1 FROM concept_parent cp JOIN concept c ON c.concept_id=cp.concept_id
    WHERE cp.parent_concept_id=4628 AND c.preferred_label=n.label
  );

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT b.id+n.seq,4628,0,n.ordinal
FROM _recreation_node n CROSS JOIN _recreation_base b
WHERE EXISTS(SELECT 1 FROM concept WHERE concept_id=b.id+n.seq);

INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector
FROM _recreation_node n CROSS JOIN _recreation_base b
WHERE EXISTS(SELECT 1 FROM concept WHERE concept_id=b.id+n.seq);

DELETE FROM source_selector
WHERE concept_id=4628 AND system_id='lcc'
  AND (selector='GV558'
       OR selector IN ('GV1853.3.C22','GV1853.3.F62')
       OR selector LIKE 'SK%'
       OR selector='GV561-1198.995');

DROP TABLE _recreation_node;
DROP TABLE _recreation_base;
COMMIT;
