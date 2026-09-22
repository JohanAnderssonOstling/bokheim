BEGIN;

-- All named parent selectors fall in the established Labor — General leaf.
-- Preserve only the literal published umbrella notation on the parent.
UPDATE lcc_selector SET concept_id=12354
WHERE concept_id=5234 AND selector<>'HD4801-8943';

COMMIT;
