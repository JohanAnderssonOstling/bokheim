BEGIN;

-- The existing children partition BS410–BS680. The nonleaf umbrella must not
-- retain permissive ranges that overlap all of them.
DELETE FROM lcc_range WHERE concept_id=5731;

UPDATE lcc_selector SET concept_id=12374
WHERE concept_id=5731 AND selector IN (
 'BS410','BS411','BS415.3','BS415.5','BS416','BS417','BS418','BS419',
 'BS432','BS435','BS440');

UPDATE lcc_selector SET concept_id=12377
WHERE concept_id=5731 AND selector IN ('BS539','BS540','BS544');

UPDATE lcc_selector SET concept_id=12378
WHERE concept_id=5731 AND selector IN ('BS559','BS560');

UPDATE lcc_selector SET concept_id=12380
WHERE concept_id=5731 AND selector IN
 ('BS615','BS617.7','BS617.8','BS618','BS619');

COMMIT;
