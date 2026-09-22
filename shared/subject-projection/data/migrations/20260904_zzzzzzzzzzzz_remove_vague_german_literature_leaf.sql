-- Remove a vague, selector-only German-literature leaf. Its LCC range remains
-- covered by the broader History of German literature parent selector.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept
WHERE concept_id='bokheim:lcc-detail:cb30e28e-e5d9-52f0-aecd-7000e6715536';

COMMIT;
