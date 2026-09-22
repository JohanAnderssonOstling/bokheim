BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5389,'BF309-BF499');

UPDATE lcc_selector SET concept_id=12873
WHERE concept_id=5389 AND selector IN
 ('BF309','BF311','BF313','BF314','BF315','BF315.2','BF315.5','BF316',
  'BF316.6','BF317','BF317.5','BF318.5');

UPDATE lcc_selector SET concept_id=12874
WHERE concept_id=5389 AND selector IN
 ('BF319-BF319.5','BF321','BF322-BF322.5','BF325','BF327');

UPDATE lcc_selector SET concept_id=12875
WHERE concept_id=5389 AND selector IN
 ('BF335-BF335.5','BF335.4-BF335.5','BF341-BF346','BF353','BF355','BF357');

UPDATE lcc_selector SET concept_id=12876
WHERE concept_id=5389 AND selector IN
 ('BF365-BF395','BF367','BF390');

UPDATE lcc_selector SET concept_id=12877
WHERE concept_id=5389 AND selector IN
 ('BF408-BF426','BF412','BF418','BF423','BF426');

UPDATE lcc_selector SET concept_id=12878
WHERE concept_id=5389 AND selector IN
 ('BF455-BF463','BF458','BF467','BF468','BF469','BF471','BF475','BF482',
  'BF485','BF491','BF495-BF499','BF497','BF498');

-- BF481 is explicitly "Work" in LCC. Reuse the established work-psychology
-- concept and expose it from Cognition as well as Applied Psychology.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (5410,5389,1);

UPDATE lcc_selector SET concept_id=5410
WHERE concept_id=5389 AND selector='BF481';

DELETE FROM lcc_range
WHERE concept_id=5389 AND start_code='BF309' AND end_code='BF499';

COMMIT;
