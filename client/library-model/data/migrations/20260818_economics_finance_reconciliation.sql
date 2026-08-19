-- Reconcile the parallel BISAC/curated and LCC Economics and Finance routes.
-- Keep Economics & Business as the visible umbrella and the existing BISAC
-- Finance concept as the canonical Finance concept. Preserve all LCC selectors
-- and descendants. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Merge the LCC HG Finance concept into the existing Business Finance concept.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 'lcc', 'HG', 1),
    ('unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 'lcc', 'HG1..HG9999@1', 2);

DELETE FROM concept_parent
WHERE parent_concept_id = 'unified:6e4eef60-d40b-5222-ab5c-8b2b81d78e0c';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:14251312-253e-5e28-9505-ee4431a5a298', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 20),
    ('unified:9480e316-8c61-5ff2-b688-c6ed5f447540', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 30),
    ('unified:923a95fb-d3dd-58f9-bdd4-24a455f34833', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 40),
    ('unified:9c9bacae-d83c-510a-838c-3c5f16df8f57', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 50),
    ('unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 60),
    ('unified:97250e66-b3b2-540a-b650-911ee2d328e0', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 70),
    ('unified:eb1d5a32-9a6d-536e-b1e2-e604c70d5e2c', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 80),
    ('unified:74d7da2f-5b75-5fde-a35c-89bbcc391aee', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 90),
    ('unified:d956d6d6-e4a5-55ac-977b-8ab0f5c5d74b', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 100),
    ('unified:6bc71771-38d1-50be-ab8c-305c844f9c72', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 110),
    ('unified:152efcb9-35f0-53ac-a2c1-33cadd7e6cc8', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 120),
    ('unified:9ccf143d-df24-5eff-a307-8362aaf4a4cc', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 130);

DELETE FROM concept
WHERE concept_id = 'unified:6e4eef60-d40b-5222-ab5c-8b2b81d78e0c';

-- Move the remaining LCC groupings into the canonical Economics & Business
-- umbrella, then remove the redundant Economics, Business & Labor wrapper.
DELETE FROM concept_parent
WHERE parent_concept_id = 'unified:371b8dde-a1ce-53ac-b7c9-fe965dff171b';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:98c6ca29-056c-5b82-83e7-aebaaa8169f9', 'unified:103d333e-4f6d-5d1c-9c4b-950677c8d681', 0, 13),
    ('unified:ae997d9d-393b-5c24-be0e-70ce3ac19e14', 'unified:103d333e-4f6d-5d1c-9c4b-950677c8d681', 0, 14),
    ('unified:2fc32450-d6ba-5776-8668-8a1566dec875', 'unified:103d333e-4f6d-5d1c-9c4b-950677c8d681', 0, 15),
    ('unified:e1e1a593-e41e-5a0d-9243-65711bec3cec', 'unified:103d333e-4f6d-5d1c-9c4b-950677c8d681', 0, 16);

DELETE FROM concept
WHERE concept_id = 'unified:371b8dde-a1ce-53ac-b7c9-fe965dff171b';

COMMIT;
