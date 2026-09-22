-- The Victorian period has a clean one-to-one LCC range.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_range(start_code, end_code, concept_id)
VALUES ('DA550', 'DA565', 3533);

COMMIT;
