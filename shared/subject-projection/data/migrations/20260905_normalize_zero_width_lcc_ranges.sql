-- A range whose endpoints are identical is an exact selector. Keeping Cutter
-- codes as numeric ranges discards the Cutter and creates false ownership
-- collisions between unrelated leaves sharing the same class number.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE normalized_exact(
 concept_id TEXT NOT NULL,
 selector TEXT NOT NULL,
 PRIMARY KEY(concept_id,selector)
) WITHOUT ROWID;

INSERT OR IGNORE INTO normalized_exact(concept_id,selector)
SELECT concept_id,substr(selector,1,instr(selector,'..')-1)
FROM source_selector
WHERE system_id='lcc'
  AND instr(selector,'..')>0
  AND instr(selector,'@')>0
  AND substr(selector,1,instr(selector,'..')-1)
      = substr(substr(selector,instr(selector,'..')+2),1,
               instr(substr(selector,instr(selector,'..')+2),'@')-1);

DELETE FROM source_selector
WHERE system_id='lcc'
  AND instr(selector,'..')>0
  AND instr(selector,'@')>0
  AND substr(selector,1,instr(selector,'..')-1)
      = substr(substr(selector,instr(selector,'..')+2),1,
               instr(substr(selector,instr(selector,'..')+2),'@')-1);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT n.concept_id,'lcc',n.selector,
       COALESCE((SELECT max(s.ordinal)+1 FROM source_selector s
                 WHERE s.concept_id=n.concept_id AND s.system_id='lcc'),0)
       + row_number() OVER(PARTITION BY n.concept_id ORDER BY n.selector)-1
FROM normalized_exact n
WHERE NOT EXISTS (
 SELECT 1 FROM source_selector s
 WHERE s.concept_id=n.concept_id AND s.system_id='lcc' AND s.selector=n.selector
);

DROP TABLE normalized_exact;
COMMIT;
