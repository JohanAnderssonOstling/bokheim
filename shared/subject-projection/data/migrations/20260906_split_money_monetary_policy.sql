-- Replace overlapping HG money ranges with six mutually exclusive broad leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:money:general','Money & Liquidity'),
 ('bokheim:money:precious-metals-coins','Precious Metals & Coinage'),
 ('bokheim:money:mints-counterfeiting','Mints & Counterfeiting'),
 ('bokheim:money:paper-legal-tender','Paper Money & Legal Tender'),
 ('bokheim:money:systems-standards','Monetary Systems & Standards'),
 ('bokheim:money:by-country','Money & Monetary Policy by Country');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:money:general','unified:29da64de-ac3b-562e-bec9-9a652d99529a',0,100),
 ('bokheim:money:precious-metals-coins','unified:29da64de-ac3b-562e-bec9-9a652d99529a',0,200),
 ('bokheim:money:mints-counterfeiting','unified:29da64de-ac3b-562e-bec9-9a652d99529a',0,300),
 ('bokheim:money:paper-legal-tender','unified:29da64de-ac3b-562e-bec9-9a652d99529a',0,400),
 ('bokheim:money:systems-standards','unified:29da64de-ac3b-562e-bec9-9a652d99529a',0,500),
 ('bokheim:money:by-country','unified:29da64de-ac3b-562e-bec9-9a652d99529a',0,600);
DELETE FROM source_selector WHERE concept_id='unified:29da64de-ac3b-562e-bec9-9a652d99529a';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:money:general','bisac','BUS045000',0),
 ('bokheim:money:general','lcc','HG178',0),
 ('bokheim:money:general','lcc','HG201..HG257.9@3',1),
 ('bokheim:money:precious-metals-coins','lcc','HG258..HG320.9@3',0),
 ('bokheim:money:mints-counterfeiting','lcc','HG321..HG347.9@3',0),
 ('bokheim:money:paper-legal-tender','lcc','HG348..HG380.9@3',0),
 ('bokheim:money:systems-standards','lcc','HG381..HG450.9@3',0),
 ('bokheim:money:by-country','lcc','HG451..HG1496@3',0);
COMMIT;
