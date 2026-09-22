-- Swedish monarch periods already include their dates in the subject labels.
-- Place them directly under Swedish History instead of nesting them below
-- By Period / 20th Century.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept_parent
SET parent_concept_id=3584,
    ordinal=CASE concept_id
        WHEN 51392 THEN 1
        WHEN 51393 THEN 2
        WHEN 51394 THEN 3
    END
WHERE parent_concept_id=11557
  AND concept_id IN (51392,51393,51394);

COMMIT;
