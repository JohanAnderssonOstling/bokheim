PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 96. Comparative intellectual-property law.
INSERT INTO concept VALUES ('bokheim:comparative-ip:general','International Intellectual Property & WIPO'),('bokheim:comparative-ip:copyright','Comparative & International Copyright Law'),('bokheim:comparative-ip:industrial','Comparative Industrial Property, Patent & Design Law');
INSERT INTO concept_parent VALUES ('bokheim:comparative-ip:general','bokheim:law:comparative:ip',0,100),('bokheim:comparative-ip:copyright','bokheim:law:comparative:ip',0,200),('bokheim:comparative-ip:industrial','bokheim:law:comparative:ip',0,300);
DELETE FROM source_selector WHERE concept_id='bokheim:law:comparative:ip';
INSERT INTO source_selector VALUES ('bokheim:comparative-ip:general','lcc','K1401..K1410.9@3',0),('bokheim:comparative-ip:copyright','lcc','K1411..K1485@3',0),('bokheim:comparative-ip:industrial','lcc','K1485.1..K1578.25@3',0);

COMMIT;
