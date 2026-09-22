BEGIN;

UPDATE lcc_selector SET concept_id=12847
WHERE concept_id=5071 AND selector IN
 ('GN700','GN701','GN705','GN710','GN720-GN722','GN733-GN740','GN741',
  'GN743','GN744','GN746','GN750-GN751','GN755','GN761','GN766');

UPDATE lcc_selector SET concept_id=12848
WHERE concept_id=5071 AND selector='GN768-GN780.52';

UPDATE lcc_selector SET concept_id=12849
WHERE concept_id=5071 AND selector='GN783-GN799';

UPDATE lcc_selector SET concept_id=12850
WHERE concept_id=5071 AND selector='GN800-GN802';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5071,'GN700-GN890');

DELETE FROM lcc_range
WHERE concept_id=12847 AND start_code='GN700' AND end_code='GN890';

COMMIT;
