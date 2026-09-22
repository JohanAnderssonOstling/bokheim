-- Six broad, disjoint cognition leaves following BF309-BF499.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:cognition:general','General Cognition'),
 ('bokheim:cognition:learning-attention','Learning, Attention & Apperception'),
 ('bokheim:cognition:habit-nature-nurture','Habit, Adjustment & Nature–Nurture'),
 ('bokheim:cognition:association-ideas','Association & Reproduction of Ideas'),
 ('bokheim:cognition:creativity','Creativity, Imagination & Invention'),
 ('bokheim:cognition:meaning-synesthesia','Psycholinguistics, Meaning & Synesthesia');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:cognition:general','unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02',0,100),
 ('bokheim:cognition:learning-attention','unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02',0,200),
 ('bokheim:cognition:habit-nature-nurture','unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02',0,300),
 ('bokheim:cognition:association-ideas','unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02',0,400),
 ('bokheim:cognition:creativity','unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02',0,500),
 ('bokheim:cognition:meaning-synesthesia','unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02',0,600);
DELETE FROM source_selector WHERE concept_id='unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:cognition:general','lcc','BF309..BF318.9@4',0),
 ('bokheim:cognition:learning-attention','lcc','BF319..BF334.9@4',0),
 ('bokheim:cognition:habit-nature-nurture','lcc','BF335..BF364.9@4',0),
 ('bokheim:cognition:association-ideas','lcc','BF365..BF407.9@4',0),
 ('bokheim:cognition:creativity','lcc','BF408..BF454.9@4',0),
 ('bokheim:cognition:meaning-synesthesia','lcc','BF455..BF499@4',0);
COMMIT;
