-- Add broad regional labor subjects directly beneath Labor, Work & Working Class.
-- Other-region codes follow Table H8 plus HD8100; US uses its explicit schedule.
-- General, cross-region, and unresolved geography stays on the existing parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept VALUES('bokheim:labor:region:europe','Labor in Europe');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:labor:region:europe','unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab',0,110);
INSERT INTO source_selector VALUES('bokheim:labor:region:europe','lcc','HD8371..HD8650.6@5',0);
INSERT INTO concept VALUES('bokheim:labor:region:north-america','Labor in North America');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:labor:region:north-america','unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab',0,120);
INSERT INTO source_selector VALUES('bokheim:labor:region:north-america','lcc','HD8051..HD8085@5',0);
INSERT INTO source_selector VALUES('bokheim:labor:region:north-america','lcc','HD8101..HD8110@5',1);
INSERT INTO source_selector VALUES('bokheim:labor:region:north-america','lcc','HD8942.5..HD8942.5@5',2);
INSERT INTO concept VALUES('bokheim:labor:region:asia','Labor in Asia');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:labor:region:asia','unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab',0,130);
INSERT INTO source_selector VALUES('bokheim:labor:region:asia','lcc','HD8651..HD8755@5',0);
INSERT INTO concept VALUES('bokheim:labor:region:latin-america-caribbean','Labor in Latin America & the Caribbean');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:labor:region:latin-america-caribbean','unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab',0,140);
INSERT INTO source_selector VALUES('bokheim:labor:region:latin-america-caribbean','lcc','HD8110.5..HD8370@5',0);
INSERT INTO concept VALUES('bokheim:labor:region:africa','Labor in Africa');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:labor:region:africa','unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab',0,150);
INSERT INTO source_selector VALUES('bokheim:labor:region:africa','lcc','HD8771..HD8837@5',0);
INSERT INTO concept VALUES('bokheim:labor:region:oceania','Labor in Oceania');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:labor:region:oceania','unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab',0,160);
INSERT INTO source_selector VALUES('bokheim:labor:region:oceania','lcc','HD8841..HD8850@5',0);
INSERT INTO source_selector VALUES('bokheim:labor:region:oceania','lcc','HD8930.5..HD8939.5@5',1);
COMMIT;
