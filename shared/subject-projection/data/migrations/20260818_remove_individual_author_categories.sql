-- Remove generic LCC leaf categories for individual authors. Their books fall
-- back to broader selector-backed language, period, or literature ancestors.
-- No named-author concepts are present. This migration is idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept
WHERE lower(preferred_label) LIKE '%individual author%';

COMMIT;
