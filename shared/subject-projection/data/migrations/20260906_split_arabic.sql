-- Split Arabic philology, language structure, and literature into disjoint leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:arabic:philology','Arabic Philology'),
 ('bokheim:arabic:language-general','Arabic Language—General'),
 ('bokheim:arabic:phonology','Arabic Phonology'),
 ('bokheim:arabic:morphology-syntax','Arabic Morphology & Syntax'),
 ('bokheim:arabic:etymology','Arabic Etymology & Lexicography'),
 ('bokheim:arabic:language-other','Arabic Dialects & Other Language Topics'),
 ('bokheim:arabic:literature','Arabic Literature');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:arabic:philology','unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348',0,100),
 ('bokheim:arabic:language-general','unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348',0,200),
 ('bokheim:arabic:phonology','unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348',0,300),
 ('bokheim:arabic:morphology-syntax','unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348',0,400),
 ('bokheim:arabic:etymology','unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348',0,500),
 ('bokheim:arabic:language-other','unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348',0,600),
 ('bokheim:arabic:literature','unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348',0,700);
DELETE FROM source_selector WHERE concept_id='unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:arabic:philology','lcc','PJ6001..PJ6072.9@3',0),
 ('bokheim:arabic:language-general','bisac','FOR002000',0),
 ('bokheim:arabic:language-general','lcc','PJ6073..PJ6120.9@4',0),
 ('bokheim:arabic:phonology','lcc','PJ6121..PJ6140.9@4',0),
 ('bokheim:arabic:morphology-syntax','lcc','PJ6141..PJ6171.9@4',0),
 ('bokheim:arabic:etymology','lcc','PJ6172..PJ6199.5@4',0),
 ('bokheim:arabic:language-other','lcc','PJ6199.6..PJ7144@3',0),
 ('bokheim:arabic:literature','lcc','PJ7501..PJ8517@3',0);
COMMIT;
