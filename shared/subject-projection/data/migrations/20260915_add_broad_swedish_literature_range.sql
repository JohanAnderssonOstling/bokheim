-- Give the Sweden literature node the broad LCC range from the source outline.
-- More specific child selectors remain available below it.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector (concept_id, selector)
VALUES (4473, 'PT9201-PT9999');

COMMIT;
