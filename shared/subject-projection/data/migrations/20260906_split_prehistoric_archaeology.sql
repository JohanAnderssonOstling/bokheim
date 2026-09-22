-- Consolidate overlapping exact/range selectors into five disjoint LCC leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:prehistoric-archaeology:general','General, History & Study'),
 ('bokheim:prehistoric-archaeology:period','Archaeology by Prehistoric Period'),
 ('bokheim:prehistoric-archaeology:topics','Special Topics'),
 ('bokheim:prehistoric-archaeology:museums','Museums & Exhibitions'),
 ('bokheim:prehistoric-archaeology:regions','Prehistoric Archaeology by Region');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:prehistoric-archaeology:general','unified:6a282fb5-7d78-5472-8629-811bfc81173f',0,100),
 ('bokheim:prehistoric-archaeology:period','unified:6a282fb5-7d78-5472-8629-811bfc81173f',0,200),
 ('bokheim:prehistoric-archaeology:topics','unified:6a282fb5-7d78-5472-8629-811bfc81173f',0,300),
 ('bokheim:prehistoric-archaeology:museums','unified:6a282fb5-7d78-5472-8629-811bfc81173f',0,400),
 ('bokheim:prehistoric-archaeology:regions','unified:6a282fb5-7d78-5472-8629-811bfc81173f',0,500);
DELETE FROM source_selector WHERE concept_id='unified:6a282fb5-7d78-5472-8629-811bfc81173f';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:prehistoric-archaeology:general','lcc','GN700..GN767.9@4',0),
 ('bokheim:prehistoric-archaeology:general','lcc','GN700-890',1),
 ('bokheim:prehistoric-archaeology:period','lcc','GN768..GN782.9@4',0),
 ('bokheim:prehistoric-archaeology:topics','lcc','GN783..GN799.9@4',0),
 ('bokheim:prehistoric-archaeology:museums','lcc','GN800..GN802.9@4',0),
 ('bokheim:prehistoric-archaeology:regions','lcc','GN803..GN890@4',0);
COMMIT;
