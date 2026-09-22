PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 77. Bible translations: group the BISAC translation-by-edition matrix into useful version families.
INSERT INTO concept VALUES ('bokheim:bible-versions:kjv','King James & New King James Versions'),('bokheim:bible-versions:niv','New International Version Family'),('bokheim:bible-versions:spanish','Spanish Bible Translations'),('bokheim:bible-versions:literal','Literal Modern English Translations'),('bokheim:bible-versions:readable','Contemporary & Readable English Translations'),('bokheim:bible-versions:catholic','Catholic Bible Translations'),('bokheim:bible-versions:multiple','Multiple Bible Translations'),('bokheim:bible-versions:other','Other Bible Translations');
INSERT INTO concept_parent VALUES ('bokheim:bible-versions:kjv','bokheim:bible:translations',0,100),('bokheim:bible-versions:niv','bokheim:bible:translations',0,200),('bokheim:bible-versions:spanish','bokheim:bible:translations',0,300),('bokheim:bible-versions:literal','bokheim:bible:translations',0,400),('bokheim:bible-versions:readable','bokheim:bible:translations',0,500),('bokheim:bible-versions:catholic','bokheim:bible:translations',0,600),('bokheim:bible-versions:multiple','bokheim:bible:translations',0,700),('bokheim:bible-versions:other','bokheim:bible:translations',0,800);
UPDATE source_selector SET concept_id=CASE
 WHEN substr(selector,1,6) IN ('BIB006','BIB014') THEN 'bokheim:bible-versions:kjv'
 WHEN substr(selector,1,6) IN ('BIB012','BIB013','BIB017') THEN 'bokheim:bible-versions:niv'
 WHEN substr(selector,1,6) IN ('BIB007','BIB019','BIB024') THEN 'bokheim:bible-versions:spanish'
 WHEN substr(selector,1,6) IN ('BIB001','BIB003','BIB010','BIB016') THEN 'bokheim:bible-versions:literal'
 WHEN substr(selector,1,6) IN ('BIB002','BIB004','BIB005','BIB011','BIB015','BIB020','BIB022','BIB023') THEN 'bokheim:bible-versions:readable'
 WHEN substr(selector,1,6)='BIB009' THEN 'bokheim:bible-versions:catholic'
 WHEN substr(selector,1,6)='BIB008' THEN 'bokheim:bible-versions:multiple'
 ELSE 'bokheim:bible-versions:other' END
 WHERE concept_id='bokheim:bible:translations' AND system_id='bisac';
UPDATE source_selector SET concept_id='bokheim:bible-versions:other' WHERE concept_id='bokheim:bible:translations';

COMMIT;
