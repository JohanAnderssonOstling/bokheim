BEGIN;

UPDATE lcc_selector SET concept_id=13179
WHERE concept_id=5090 AND selector IN
 ('HF1701','HF1703','HF1705','HF1711','HF1713');

UPDATE lcc_selector SET concept_id=13180
WHERE concept_id=5090 AND selector='HF1715-HF1718';

UPDATE lcc_selector SET concept_id=13181
WHERE concept_id=5090 AND selector='HF1721-HF1733';

UPDATE lcc_selector SET concept_id=13182
WHERE concept_id=5090 AND selector='HF1745-HF2580.7';

UPDATE lcc_selector SET concept_id=13183
WHERE concept_id=5090 AND selector IN ('HF2661-HF2671','HF2701');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5090,'HF1701-HF2701');

DELETE FROM lcc_range
WHERE concept_id=5090 AND start_code='HF1701' AND end_code='HF2701';

COMMIT;
