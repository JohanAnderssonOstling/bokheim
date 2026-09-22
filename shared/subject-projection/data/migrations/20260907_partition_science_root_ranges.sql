-- Broad exact subclass selectors remain as fallbacks on these roots.  Remove
-- redundant schedule-wide ranges and route the one distinct residual span.

UPDATE lcc_range SET concept_id=5028
WHERE concept_id=4780
  AND start_code='QB4'
  AND end_code='QB4.9';

DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (2377,'GE1','GE350'),
  (2377,'GE40','GE45'),
  (4780,'QB1','QB991'),
  (4904,'QE1','QE996.5'),
  (5043,'QP1','QP981'),
  (5043,'QP34','QP38')
);
