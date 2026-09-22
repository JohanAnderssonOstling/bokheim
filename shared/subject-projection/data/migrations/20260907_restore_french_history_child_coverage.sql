-- Restore French-history coverage removed with the DC umbrella selectors.
-- Source: bundled LCC reference classification spans for DC139–249, DC251–354.9,
-- DC356–424, DC425–435 (including ruler decimals), and DC600–801.
-- General & Cross-period French History supplies residual coverage across the
-- French portion of DC, stopping before Andorra (DC921+) and Monaco (DC941+).
-- Narrower ranges and exact selectors retain priority, including French Travel.
-- Existing French History remains a navigation node with its exact DC selector.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES(52289,'General & Cross-period French History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52289,3523,0);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DC','1','919',52289);
INSERT INTO concept(concept_id,preferred_label) VALUES(52290,'Revolution & Napoleon, 1789–1815');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52290,3523,0);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DC','139','249',52290);
INSERT INTO concept(concept_id,preferred_label) VALUES(52291,'19th-Century French History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52291,3523,0);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DC','251','354.9',52291);
INSERT INTO concept(concept_id,preferred_label) VALUES(52292,'20th-Century French History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52292,3523,0);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DC','356','424.9',52292);
INSERT INTO concept(concept_id,preferred_label) VALUES(52293,'21st-Century French History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52293,3523,0);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DC','425','435.9',52293);
INSERT INTO concept(concept_id,preferred_label) VALUES(52294,'French Regional & Local History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(52294,3523,0);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DC','600','801',52294);
UPDATE taxonomy_meta SET value='13' WHERE key='release_id';
COMMIT;
