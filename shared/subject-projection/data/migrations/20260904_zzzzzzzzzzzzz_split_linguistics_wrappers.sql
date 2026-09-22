-- Dissolve two synthetic linguistics wrappers and expose their externally
-- classified children directly beneath Linguistics.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept_parent
SET parent_concept_id='unified:ebdaebd5-a59a-50b3-aea4-6cfabc06d27a',
    display_order=CASE concept_id
        WHEN 'unified:e59a2511-16b6-5d79-a072-274701521294' THEN 30
        WHEN 'unified:d590381c-2c72-5284-a843-9005b8d8c5c3' THEN 31
        WHEN 'unified:c20ce08b-24e4-5ce6-ae3b-6e55002047e9' THEN 32
        WHEN 'unified:fb6d341b-2ef6-571a-a2e6-581de9660f55' THEN 40
        WHEN 'unified:d3fe8735-ba8b-5258-9499-b41b839abc82' THEN 41
        WHEN 'unified:78e7d9d7-06a9-5666-ae0f-1c39bb606ec0' THEN 42
    END
WHERE parent_concept_id IN (
    'bokheim:subject:linguistics:applied-computational-psychological',
    'bokheim:subject:linguistics:historical-comparative-philology'
);

DELETE FROM concept
WHERE concept_id IN (
    'bokheim:subject:linguistics:applied-computational-psychological',
    'bokheim:subject:linguistics:historical-comparative-philology'
);

COMMIT;
