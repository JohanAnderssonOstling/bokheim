-- Continue reviewed national-law coverage at existing Law subject 5276.
-- Published bounds from the updated local LCC reference. No new subjects.
BEGIN IMMEDIATE;
-- Netherlands
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KKM','1','9999',5276);
-- South Korea
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KPA','0','4990',5276);
-- Switzerland
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KKW','1','9999',5276);
-- Poland
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KKP','1','4999',5276);
-- Indonesia
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KNW','0','4990',5276);
-- Australia
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KU','1','4999',5276);
-- Austria
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KJJ','1','9999',5276);
-- Turkey
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KKX','1','4999',5276);
-- Ukraine
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KKY','1','4999',5276);
-- Portugal
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KKQ','1','4999',5276);
-- India
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KNS','1','4999',5276);
-- Belgium
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('KJK','1','5299',5276);
COMMIT;
