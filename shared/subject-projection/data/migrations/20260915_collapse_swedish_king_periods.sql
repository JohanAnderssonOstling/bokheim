-- Keep Swedish History / By Period / 20th Century as the single modern
-- period subject. The individual monarch subjects are redundant because the
-- 20th-century subject already covers DL860–DL879.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM lcc_range WHERE concept_id IN (51392,51393,51394);
DELETE FROM source_label WHERE concept_id IN (51392,51393,51394);
DELETE FROM concept_parent WHERE concept_id IN (51392,51393,51394)
   OR parent_concept_id IN (51392,51393,51394);
DELETE FROM concept WHERE concept_id IN (51392,51393,51394);

COMMIT;
