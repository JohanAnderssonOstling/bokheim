BEGIN;
DELETE FROM lcc_range WHERE concept_id IN (12800,12312,12205,676,12339,12247);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (12800,'HV1551-3024'),(12312,'GT2400-3390.5'),(12205,'HD9720-9975'),
 (676,'GT500-2370'),(12339,'HD3611-4730.9'),(12247,'HQ1101-1189.9');

CREATE TEMP TABLE _split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('Blindness & Visual Disability',12800,'HV1571','HV2349'),('Education of Blind People',12800,'HV1617.2','HV1708'),('Braille & Accessible Type',12800,'HV1666','HV1698'),('Accessible Books & Talking Books',12800,'HV1721','HV1756'),('Deafness & Hearing Disability',12800,'HV2350','HV2990.7'),('Education of Deaf People',12800,'HV2416.2','HV2500.2'),('Deafness by Region',12800,'HV2510','HV2990.7'),
 ('Childhood Customs',12312,'GT2450','GT2480'),('Birth & Adoption Customs',12312,'GT2460','GT2470'),('Women’s Private-Life Customs',12312,'GT2520','GT2540'),('Love, Courtship & Sexual Customs',12312,'GT2600','GT2796'),('Marriage Customs',12312,'GT2660','GT2810'),('Eating & Drinking Customs',12312,'GT2850','GT2960'),('Food & Beverage Customs',12312,'GT2855','GT2930'),('Smoking Customs',12312,'GT3020','GT3030'),('Burial & Funeral Customs',12312,'GT3150','GT3390.5'),
 ('Forest Products & Logging',12205,'HD9750','HD9769'),('Leather-Goods Industry',12205,'HD9780','HD9787'),('Graphic-Arts Equipment Industry',12205,'HD9790','HD9795'),('Office-Equipment Industry',12205,'HD9800','HD9803'),('Paper Industry',12205,'HD9820','HD9839'),('Textile Industries',12205,'HD9850','HD9869'),('Cotton Manufacturing',12205,'HD9870','HD9889'),('Wool Industry',12205,'HD9890','HD9909'),('Silk Industry',12205,'HD9910','HD9929'),('Clothing Industry',12205,'HD9940','HD9949.5'),('Dry-Goods Industry',12205,'HD9950','HD9969'),
 ('Ancient Fashion',676,'GT530','GT560'),('Fashion by Period',676,'GT530','GT596'),('Latin American Fashion',676,'GT623','GT716'),('European Fashion',676,'GT720','GT1330'),('Asian Fashion',676,'GT1370','GT1570'),('Fashion by Social Group',676,'GT1710','GT1956'),('Clothing Materials & Accessories',676,'GT2050','GT2370'),('Jewelry & Ornaments',676,'GT2250','GT2282'),('Hair & Hairstyles',676,'GT2290','GT2320'),
 ('Industrial Inspection',12339,'HD3656','HD3790.9'),('State Industries & Public Works',12339,'HD3840','HD4420.8'),('Municipal Services & Public Works',12339,'HD4421','HD4730.9'),
 ('History of Women',12247,'HQ1121','HQ1154'),('Women in Antiquity',12247,'HQ1127','HQ1139'),('Women in the Middle Ages',12247,'HQ1143','HQ1147'),('Women in the Renaissance',12247,'HQ1148','HQ1149'),('Women in the Modern Period',12247,'HQ1150','HQ1155'),('Women by Culture & Ethnicity',12247,'HQ1161','HQ1172'),('Women’s Studies Research',12247,'HQ1180','HQ1186');

INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;
