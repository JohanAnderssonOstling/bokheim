-- Preserve the meaningful distinction between ancient geographic works and
-- modern scholarship without restoring the old publication-form hierarchy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:history-lcc-full:881b5a66-4374-5d41-88d6-0ebad31c9061','Classical Geographers');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:history-lcc-full:881b5a66-4374-5d41-88d6-0ebad31c9061',
  'unified:70b82b40-785f-59ff-977a-8e9eb6d04689',0,10);

DELETE FROM source_selector
WHERE concept_id='unified:70b82b40-785f-59ff-977a-8e9eb6d04689'
  AND system_id='lcc'
  AND selector='DE27..DE27@4';

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:history-lcc-full:881b5a66-4374-5d41-88d6-0ebad31c9061',
  'lcc','DE27..DE27@4',0);

COMMIT;
