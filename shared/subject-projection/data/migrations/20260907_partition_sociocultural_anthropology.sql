BEGIN;

UPDATE lcc_selector SET concept_id=12979
WHERE concept_id=5061 AND selector IN
 ('GN301','GN302','GN303-GN304','GN307','GN307.3','GN307.5-GN307.7',
  'GN307.8-GN307.82','GN308-GN308.3','GN310-GN316','GN320','GN323',
  'GN325','GN330','GN333','GN340','GN345-GN348');

UPDATE lcc_selector SET concept_id=12980
WHERE concept_id=5061 AND selector IN
 ('GN357-GN367','GN368','GN370','GN372');

UPDATE lcc_selector SET concept_id=12981
WHERE concept_id=5061 AND selector='GN378';

UPDATE lcc_selector SET concept_id=12982
WHERE concept_id=5061 AND selector IN ('GN397-GN397.7','GN398-GN398.7');

UPDATE lcc_selector SET concept_id=5059
WHERE concept_id=5061 AND selector='GN406-GN517';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5061,'GN301-GN674');

DELETE FROM lcc_range
WHERE concept_id=12979 AND start_code='GN301' AND end_code='GN674';

COMMIT;
