PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Candidate 31 was an LCC cross-reference ("International law, see JZ and KZ"),
-- not a subject. Remove it instead of manufacturing children from redirect codes.
DELETE FROM concept WHERE concept_id='unified:5e71ac50-3e6f-5e04-bc7e-722c06cf630c';

COMMIT;
