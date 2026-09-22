-- Bare QA remains the exact Mathematics fallback.  Route its precise residual
-- spans to existing subjects and remove the schedule-wide umbrella range.

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 9005,5721,coalesce(max(ordinal),-1)+1 FROM concept_parent WHERE concept_id=9005;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 10932,5716,coalesce(max(ordinal),-1)+1 FROM concept_parent WHERE concept_id=10932;

UPDATE lcc_range SET concept_id=4965
WHERE concept_id=4916
  AND start_code='QA274'
  AND end_code='QA274.9';

UPDATE lcc_range SET concept_id=9005
WHERE concept_id=4916
  AND start_code='QA297'
  AND end_code='QA299.4';

UPDATE lcc_selector SET concept_id=10932
WHERE concept_id=4916 AND selector='QA8.9-QA10.3';

UPDATE lcc_selector SET concept_id=5722
WHERE concept_id=4916 AND selector='QA10.92-QA20';

DELETE FROM lcc_range
WHERE concept_id=4916
  AND start_code='QA1'
  AND end_code='QA939';
