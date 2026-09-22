BEGIN;

UPDATE lcc_selector SET concept_id=13001
WHERE concept_id=5070 AND selector IN
 ('GN49','GN50','GN50.2','GN50.3','GN50.4-GN50.45','GN50.5-GN50.6',
  'GN50.8-GN59','GN51-GN59','GN59.6','GN59.8-GN59.82','GN60','GN62');

UPDATE lcc_selector SET concept_id=13002
WHERE concept_id=5070 AND selector='GN62.8-GN265';

UPDATE lcc_selector SET concept_id=13003
WHERE concept_id=5070 AND selector IN ('GN269-GN279','GN280.7');

UPDATE lcc_selector SET concept_id=13004
WHERE concept_id=5070 AND selector IN ('GN281-GN289','GN293');

UPDATE lcc_selector SET concept_id=13005
WHERE concept_id=5070 AND selector IN ('GN296-GN296.5','GN298');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5070,'GN49-GN298');

DELETE FROM lcc_range
WHERE concept_id=5070 AND start_code='GN49' AND end_code='GN298';

COMMIT;
