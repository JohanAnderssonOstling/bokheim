PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;
INSERT OR IGNORE INTO concept VALUES('bokheim:religion:rationalism','Rationalism, Atheism & Freethought');
INSERT OR IGNORE INTO concept_parent VALUES('bokheim:religion:rationalism','bokheim:subject:religion:general-reference',28,28);
DELETE FROM source_selector WHERE concept_id='bokheim:subject:religion:general-reference' AND selector='BL1..BL2685.2@2';
UPDATE source_selector SET concept_id='bokheim:religion:rationalism' WHERE concept_id='bokheim:subject:religion:general-reference' AND system_id='lcc' AND CAST(substr(selector,3) AS REAL)>=2700;
INSERT OR IGNORE INTO source_selector VALUES
('bokheim:subject:religion:general-reference','lcc','BL1..BL50@2',10000),
('unified:c0e05f1f-c514-544e-ba2c-7bf5d67e2081','lcc','BL51..BL52.99@2',10000),
('bokheim:philosophy-lcc-full:676d073f-65c4-58df-8430-0c9f1efb0519','lcc','BL53..BL54.99@2',10000),
('bokheim:religion:society-culture','lcc','BL55..BL69.99@2',10000),
('bokheim:religion:sacred-texts','lcc','BL70..BL73.99@2',10000),
('bokheim:religion:world-religions','lcc','BL74..BL99.99@2',10000),
('bokheim:religion:natural-theology','lcc','BL100..BL299.99@2',10000),
('bokheim:religion:mythology','lcc','BL300..BL349.99@2',10000),
('bokheim:religion:comparative','lcc','BL350..BL424.99@2',10000),
('bokheim:religion:doctrine','lcc','BL425..BL499.99@2',10000),
('bokheim:religion:eschatology','lcc','BL500..BL549.99@2',10000),
('bokheim:religion:worship','lcc','BL550..BL623.99@2',10000),
('bokheim:religion:life','lcc','BL624..BL629.99@2',10000),
('bokheim:religion:organization','lcc','BL630..BL659.99@2',10000),
('bokheim:religion:european','lcc','BL660..BL999.99@2',10000),
('bokheim:religion:asian-regional','lcc','BL1000..BL1099.99@2',10000),
('unified:570e207b-a7f0-5d41-b376-a47a2a501531','lcc','BL1100..BL1299.99@2',10000),
('unified:caac7333-35b2-59a4-839e-112e46474360','lcc','BL1300..BL1380@2',10000),
('unified:77c2368e-6099-50b6-9993-a0ca18af72c8','lcc','BL1500..BL1599.99@2',10000),
('bokheim:religion:ancient-near-east','lcc','BL1600..BL1699.99@2',10000),
('bokheim:religion:asian-regional','lcc','BL1700..BL2389.99@2',10001),
('bokheim:religion:african','lcc','BL2390..BL2499.99@2',10000),
('bokheim:religion:american-indigenous','lcc','BL2500..BL2599.99@2',10000),
('bokheim:religion:pacific','lcc','BL2600..BL2669.99@2',10000),
('bokheim:religion:arctic','lcc','BL2670..BL2699.99@2',10000),
('bokheim:religion:rationalism','lcc','BL2700..BL2790@2',10000);
COMMIT;
