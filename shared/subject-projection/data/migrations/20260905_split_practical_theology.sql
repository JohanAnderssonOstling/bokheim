-- Replace exhaustive BV detail on Practical Theology with its six broad,
-- reader-meaningful LCC divisions. Practical Theology remains the fallback.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:religion:practical-theology:worship','Worship'),
 ('bokheim:religion:practical-theology:ecclesiastical','Ecclesiastical Theology'),
 ('bokheim:religion:practical-theology:missions','Missions'),
 ('bokheim:religion:practical-theology:evangelism','Evangelism & Revivals'),
 ('bokheim:religion:practical-theology:pastoral','Pastoral Theology'),
 ('bokheim:religion:practical-theology:christian-life','Christian Life & Devotion');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:religion:practical-theology:worship','unified:6a45d882-0ece-5f64-abe8-09af8fd640e9',0,10),
 ('bokheim:religion:practical-theology:ecclesiastical','unified:6a45d882-0ece-5f64-abe8-09af8fd640e9',0,20),
 ('bokheim:religion:practical-theology:missions','unified:6a45d882-0ece-5f64-abe8-09af8fd640e9',0,30),
 ('bokheim:religion:practical-theology:evangelism','unified:6a45d882-0ece-5f64-abe8-09af8fd640e9',0,40),
 ('bokheim:religion:practical-theology:pastoral','unified:6a45d882-0ece-5f64-abe8-09af8fd640e9',0,50),
 ('bokheim:religion:practical-theology:christian-life','unified:6a45d882-0ece-5f64-abe8-09af8fd640e9',0,60);

DELETE FROM source_selector
WHERE concept_id='unified:6a45d882-0ece-5f64-abe8-09af8fd640e9'
  AND system_id='lcc'
  AND selector NOT IN ('BV','BV1..BV5099@1');

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:religion:practical-theology:worship','lcc','BV5..BV530@2',0),
 ('bokheim:religion:practical-theology:ecclesiastical','lcc','BV590..BV1652@2',0),
 ('bokheim:religion:practical-theology:missions','lcc','BV2000..BV3705@2',0),
 ('bokheim:religion:practical-theology:evangelism','lcc','BV3750..BV3799@2',0),
 ('bokheim:religion:practical-theology:pastoral','lcc','BV4000..BV4470@2',0),
 ('bokheim:religion:practical-theology:christian-life','lcc','BV4485..BV5099@2',0);

COMMIT;
