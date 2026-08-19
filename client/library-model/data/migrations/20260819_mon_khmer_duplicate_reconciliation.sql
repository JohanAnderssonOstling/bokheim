-- Merge the generated LCC-detail duplicate into the existing unified concept.
-- The broader selector is retained while the duplicate sibling is removed, so
-- runtime taxonomy validation and LCC coverage both remain intact.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal)
VALUES ('unified:3e4ea650-5415-5d93-ac03-e178cd6c0135', 'lcc', 'PL4301..PL4470@4', 2);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
VALUES ('unified:3e4ea650-5415-5d93-ac03-e178cd6c0135', 'Mon-Khmer (Mon-Anam) languages', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:lcc-detail:29daeac5-c3e8-554f-a836-8467f371210d';

COMMIT;
