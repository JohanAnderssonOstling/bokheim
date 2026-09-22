-- Hide detail below United States history cities while retaining it in the
-- master taxonomy for classification. This migration is idempotent.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS browse_exclusion (
    concept_id TEXT PRIMARY KEY REFERENCES concept(concept_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
WITH RECURSIVE city(concept_id) AS (
    VALUES
        ('bokheim:history-lcc-full:297c0734-cc3a-5d2f-b225-1d31fa700219'),
        ('bokheim:history-lcc-full:b61054c0-b584-5474-ab45-f9027c1d0e88'),
        ('bokheim:history-lcc-full:642909f3-ba20-51bc-b00a-81cca93ac548'),
        ('bokheim:history-lcc-full:a5d3f82e-cdc3-51d0-812f-206d47bdeeae')
), descendant(concept_id) AS (
    SELECT child.concept_id FROM concept_parent child JOIN city ON city.concept_id = child.parent_concept_id
    UNION
    SELECT child.concept_id FROM concept_parent child JOIN descendant parent ON parent.concept_id = child.parent_concept_id
)
INSERT OR IGNORE INTO browse_exclusion (concept_id) SELECT concept_id FROM descendant;
COMMIT;
