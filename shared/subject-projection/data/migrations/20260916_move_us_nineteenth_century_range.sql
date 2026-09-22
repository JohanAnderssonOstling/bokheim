-- E337.5–E337.999 is the general U.S. nineteenth-century range, not
-- Constitutional Era coverage.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE lcc_range
SET concept_id = 3404
WHERE concept_id = 3403
  AND start_code = 'E337.5'
  AND end_code = 'E337.999';

COMMIT;
