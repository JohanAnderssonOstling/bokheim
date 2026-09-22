-- Replace exhaustive management detail with broad, high-use reader subjects.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:management:general-strategy','General & Strategic Management'),
 ('bokheim:management:operations','Operations, Productivity & Supply Chains'),
 ('bokheim:management:innovation','Innovation & Technology Management'),
 ('bokheim:management:risk','Risk & Crisis Management'),
 ('bokheim:management:organization','Organizations, Teams & Change'),
 ('bokheim:management:responsibility','Corporate Responsibility'),
 ('bokheim:management:enterprise','Enterprise & Project Management');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:management:general-strategy','unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b',0,10),
 ('bokheim:management:operations','unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b',0,20),
 ('bokheim:management:innovation','unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b',0,30),
 ('bokheim:management:risk','unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b',0,40),
 ('bokheim:management:organization','unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b',0,50),
 ('bokheim:management:responsibility','unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b',0,60),
 ('bokheim:management:enterprise','unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b',0,70);

DELETE FROM source_selector
WHERE concept_id='unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b'
  AND system_id='lcc' AND selector<>'HD28..HD70@2';

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:management:general-strategy','lcc','HD28..HD37@3',0),
 ('bokheim:management:operations','lcc','HD38..HD38.5@3',0),
 ('bokheim:management:operations','lcc','HD56..HD57.5@3',1),
 ('bokheim:management:innovation','lcc','HD45..HD45.2@3',0),
 ('bokheim:management:risk','lcc','HD49..HD49.5@3',0),
 ('bokheim:management:risk','lcc','HD61..HD61.5@3',1),
 ('bokheim:management:organization','lcc','HD58.7..HD58.95@3',0),
 ('bokheim:management:organization','lcc','HD66..HD66.5@3',1),
 ('bokheim:management:responsibility','lcc','HD60..HD60.5@3',0),
 ('bokheim:management:enterprise','lcc','HD62.2..HD62.8@3',0),
 ('bokheim:management:enterprise','lcc','HD69.P75',1);

COMMIT;
