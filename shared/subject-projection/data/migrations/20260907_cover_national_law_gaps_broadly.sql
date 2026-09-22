-- Cover reviewed national-law schedule gaps at the existing Law subject.
-- Bounds are evidenced in lcc-mdsconnect-2016.sqlite3, with reference updates.
-- Add no subjects; existing more-specific assignments continue to take precedence.
BEGIN IMMEDIATE;
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
 ('KKT','1','9999',5276), -- Spain
 ('KKH','1','9860',5276), -- Italy (excludes the separately scheduled Kosovo range)
 ('KNX','0','4999',5276), -- Japan
 ('KLB','0','5780',5276), -- Russia
 ('KHD','0','9900',5276); -- Brazil
COMMIT;
