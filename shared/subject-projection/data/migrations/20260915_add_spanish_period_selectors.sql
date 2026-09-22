-- Restore the source selectors for Spanish regional literature periods.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (11951,'PQ7797'),
 (11956,'PQ7297'),
 (11965,'PQ8097'),
 (11978,'PQ8497');

COMMIT;
