PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;
INSERT OR IGNORE INTO concept VALUES
('bokheim:politics:us:administration','Federal Administration'),('bokheim:politics:us:executive','Presidency & Executive Branch'),('bokheim:politics:us:congress','Congress & Legislative Branch'),('bokheim:politics:us:participation','Political Rights & Participation'),('bokheim:politics:us:parties','Political Parties'),('bokheim:politics:us:states','State & Local Government'),('bokheim:politics:us:territories','Territories & Insular Government');
WITH x(id,o) AS (VALUES('bokheim:politics:us:administration',1),('bokheim:politics:us:executive',2),('bokheim:politics:us:congress',3),('bokheim:politics:us:participation',4),('bokheim:politics:us:parties',5),('bokheim:politics:us:states',6),('bokheim:politics:us:territories',7)) INSERT OR IGNORE INTO concept_parent SELECT id,'unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f',o,o FROM x;
-- Move all JK ownership from the generic JF node first.
UPDATE source_selector SET concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f' WHERE concept_id='unified:d559fba0-8aae-58dd-9df0-7e459aef7cb2' AND system_id='lcc' AND selector LIKE 'JK%';
-- Remove generated ranges that cross the new child boundaries.
DELETE FROM source_selector WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f' AND selector IN ('JK1..JK9593@2','JK1..JK9993@1','JK401..JK1685@4','JK404..JK1685@3');
UPDATE source_selector SET concept_id=CASE WHEN CAST(substr(selector,3) AS REAL) BETWEEN 401 AND 500.99 OR CAST(substr(selector,3) AS REAL) BETWEEN 1440 AND 1685.99 THEN 'bokheim:politics:us:administration' WHEN CAST(substr(selector,3) AS REAL) BETWEEN 501 AND 899.99 THEN 'bokheim:politics:us:executive' WHEN CAST(substr(selector,3) AS REAL) BETWEEN 900 AND 1439.99 THEN 'bokheim:politics:us:congress' WHEN CAST(substr(selector,3) AS REAL) BETWEEN 1717 AND 2249.99 THEN 'bokheim:politics:us:participation' WHEN CAST(substr(selector,3) AS REAL) BETWEEN 2250 AND 2399.99 THEN 'bokheim:politics:us:parties' WHEN CAST(substr(selector,3) AS REAL) BETWEEN 2400 AND 9599.99 THEN 'bokheim:politics:us:states' WHEN CAST(substr(selector,3) AS REAL)>=9660 THEN 'bokheim:politics:us:territories' ELSE concept_id END WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f' AND system_id='lcc';
-- Disjoint ranges preserve broad incoming JK coverage without overlap.
INSERT OR IGNORE INTO source_selector VALUES
('unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f','lcc','JK1..JK400@2',10000),
('bokheim:politics:us:administration','lcc','JK401..JK500@2',10000),
('bokheim:politics:us:executive','lcc','JK501..JK899@2',10000),
('bokheim:politics:us:congress','lcc','JK900..JK1439@2',10000),
('bokheim:politics:us:administration','lcc','JK1440..JK1685@2',10001),
('bokheim:politics:us:participation','lcc','JK1717..JK2249@2',10000),
('bokheim:politics:us:parties','lcc','JK2250..JK2399@2',10000),
('bokheim:politics:us:states','lcc','JK2400..JK9599@2',10000),
('bokheim:politics:us:territories','lcc','JK9660..JK9993@2',10000);
COMMIT;
