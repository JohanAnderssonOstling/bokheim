-- Second batch of exact official LCC leaf-span consolidations.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _leaf_span2(concept_id INTEGER NOT NULL, selector TEXT NOT NULL,
                              PRIMARY KEY(concept_id,selector));
INSERT INTO _leaf_span2 VALUES
  (4379,'PN661..PN694'),
  (4856,'QH91..QH91.8'),
  (4856,'QH92..QH95.59'),
  (4946,'QA174..QA183'),
  (13328,'QA166..QA166.247'),
  (12746,'DC611.P951..DC611.P979'),
  (12748,'DC611.C8..DC611.C8365'),
  (12754,'DP302.C36..DP302.C51'),
  (12756,'DP350..DP374');

DELETE FROM _leaf_span2
WHERE EXISTS(SELECT 1 FROM concept_parent cp WHERE cp.parent_concept_id=_leaf_span2.concept_id);

-- All target concepts except Group Theory are wholly represented by their new spans.
DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN (4379,4856,13328,12746,12748,12754,12756)
  AND concept_id IN (SELECT concept_id FROM _leaf_span2);

-- QA172.4 is a valid Group Theory code immediately preceding the official QA174-QA183 span.
DELETE FROM source_selector
WHERE concept_id=4946 AND system_id='lcc' AND selector<>'QA172.4'
  AND EXISTS(SELECT 1 FROM _leaf_span2 WHERE concept_id=4946);

INSERT INTO source_selector(concept_id,system_id,selector)
SELECT concept_id,'lcc',selector FROM _leaf_span2;

DROP TABLE _leaf_span2;
COMMIT;
