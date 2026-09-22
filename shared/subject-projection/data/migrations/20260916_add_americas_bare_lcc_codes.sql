-- Class-only E and F describe the Americas. Exact bare codes only:
-- preserve existing numbered classifications and do not add E*/F* prefixes.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (3379,'E'),
 (3379,'F');
COMMIT;
