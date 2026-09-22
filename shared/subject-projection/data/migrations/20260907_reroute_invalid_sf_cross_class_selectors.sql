BEGIN;

UPDATE lcc_selector SET concept_id=5793
WHERE selector IN ('SF89','SF92')
  AND concept_id IN (13373,13351);

UPDATE lcc_selector SET concept_id=5791
WHERE selector IN ('SF460','SF460.3','SF460.4','SF460.6','SF518','SF561')
  AND concept_id IN (13344,13360,13362);

COMMIT;
