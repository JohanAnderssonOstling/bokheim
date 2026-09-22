-- Curate Austria into meaningful chronological periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE at_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id) INSERT INTO at_tree SELECT id FROM t;

-- General Austrian political history belongs at the country root.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',s.system_id,s.selector,COALESCE((SELECT max(x.ordinal)+1 FROM source_selector x WHERE x.concept_id='unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572' AND x.system_id=s.system_id),0)
FROM source_selector s WHERE s.concept_id='bokheim:history-lcc:ddeeb696-796c-5b99-9d1c-add24eaa88c7';

INSERT OR IGNORE INTO concept VALUES
('bokheim:history-lcc:db04bb03-2de2-5a31-ba91-e814231cb540','Early & Medieval Austria, to 1521'),
('bokheim:history-lcc-full:52e23e99-8fd0-5429-9210-b090ef0d821a','Margraviate & Duchy, 976–1246'),
('bokheim:austria:premyslid','Interregnum & Přemyslid Rule, 1246–1278'),
('bokheim:austria:early-habsburg','Early Habsburg Austria, 1278–1521'),
('unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9','Habsburg Monarchy, 1521–1792'),
('bokheim:history-lcc:9dcb1e55-2cad-5073-a488-3e73ff95c74e','Reformation & Thirty Years’ War, 1521–1648'),
('bokheim:history-lcc:67752989-ba16-5db3-a35e-96e53fd55bf6','Absolutist Monarchy, 1648–1740'),
('bokheim:history-lcc:94565cae-cefc-550a-87c5-a7513736999e','Theresian & Josephine Era, 1740–1792'),
('bokheim:history-lcc:ff83be8a-1c2c-5a81-b237-d5dc5d28d17b','Revolutionary & Napoleonic Era, 1792–1815'),
('bokheim:austria:empire','Austrian Empire, 1804–1867'),
('bokheim:austria:metternich','Metternich Era, 1815–1848'),
('bokheim:history-lcc:16d3ecc5-a8da-52a9-bcfa-b21609580d6b','Revolution of 1848'),
('bokheim:austria:neo-absolutism','Neo-Absolutism, 1849–1867'),
('bokheim:austria:austria-hungary','Austria-Hungary, 1867–1918'),
('bokheim:austria:first-republic','First Republic, 1918–1934'),
('bokheim:austria:austrofascism','Austrofascist State, 1934–1938'),
('bokheim:history-lcc:690bf9cb-ea63-5da0-8898-0dc7576cb6ec','Anschluss, 1938–1945'),
('bokheim:history-lcc:d06e62ee-6171-5292-ba6f-285209d4df8e','Allied Occupation, 1945–1955'),
('bokheim:history-lcc:d2e4a164-6678-5287-bc69-918460d02739','Second Republic, 1955–Present');

CREATE TEMP TABLE at_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO at_keep SELECT concept_id FROM concept WHERE concept_id='unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572' OR concept_id LIKE 'bokheim:austria:%' OR concept_id IN
('bokheim:history-lcc:db04bb03-2de2-5a31-ba91-e814231cb540','bokheim:history-lcc-full:52e23e99-8fd0-5429-9210-b090ef0d821a','unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9','bokheim:history-lcc:9dcb1e55-2cad-5073-a488-3e73ff95c74e','bokheim:history-lcc:67752989-ba16-5db3-a35e-96e53fd55bf6','bokheim:history-lcc:94565cae-cefc-550a-87c5-a7513736999e','bokheim:history-lcc:ff83be8a-1c2c-5a81-b237-d5dc5d28d17b','bokheim:history-lcc:16d3ecc5-a8da-52a9-bcfa-b21609580d6b','bokheim:history-lcc:690bf9cb-ea63-5da0-8898-0dc7576cb6ec','bokheim:history-lcc:d06e62ee-6171-5292-ba6f-285209d4df8e','bokheim:history-lcc:d2e4a164-6678-5287-bc69-918460d02739');

CREATE TEMP TABLE at_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (SELECT t.concept_id FROM at_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM at_tree) AND t.concept_id<>'unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572' UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM at_tree)) INSERT INTO at_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM at_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM at_tree WHERE concept_id NOT IN (SELECT concept_id FROM at_keep) AND concept_id NOT IN (SELECT concept_id FROM at_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM at_tree WHERE concept_id NOT IN (SELECT concept_id FROM at_keep) AND concept_id NOT IN (SELECT concept_id FROM at_shared));

CREATE TEMP TABLE at_edge(child TEXT,parent TEXT,ord INTEGER,PRIMARY KEY(child,parent)) WITHOUT ROWID;
INSERT INTO at_edge VALUES
('bokheim:history-lcc:db04bb03-2de2-5a31-ba91-e814231cb540','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',10),('unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',20),('bokheim:history-lcc:ff83be8a-1c2c-5a81-b237-d5dc5d28d17b','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',30),('bokheim:austria:empire','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',40),('bokheim:austria:austria-hungary','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',50),('bokheim:austria:first-republic','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',60),('bokheim:austria:austrofascism','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',70),('bokheim:history-lcc:690bf9cb-ea63-5da0-8898-0dc7576cb6ec','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',80),('bokheim:history-lcc:d06e62ee-6171-5292-ba6f-285209d4df8e','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',90),('bokheim:history-lcc:d2e4a164-6678-5287-bc69-918460d02739','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572',100),
('bokheim:history-lcc-full:52e23e99-8fd0-5429-9210-b090ef0d821a','bokheim:history-lcc:db04bb03-2de2-5a31-ba91-e814231cb540',10),('bokheim:austria:premyslid','bokheim:history-lcc:db04bb03-2de2-5a31-ba91-e814231cb540',20),('bokheim:austria:early-habsburg','bokheim:history-lcc:db04bb03-2de2-5a31-ba91-e814231cb540',30),
('bokheim:history-lcc:9dcb1e55-2cad-5073-a488-3e73ff95c74e','unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9',10),('bokheim:history-lcc:67752989-ba16-5db3-a35e-96e53fd55bf6','unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9',20),('bokheim:history-lcc:94565cae-cefc-550a-87c5-a7513736999e','unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9',30),
('bokheim:austria:metternich','bokheim:austria:empire',10),('bokheim:history-lcc:16d3ecc5-a8da-52a9-bcfa-b21609580d6b','bokheim:austria:empire',20),('bokheim:austria:neo-absolutism','bokheim:austria:empire',30);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child,parent,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child),0),ord FROM at_edge;

UPDATE concept SET preferred_label=CASE concept_id WHEN 'bokheim:history-lcc:db04bb03-2de2-5a31-ba91-e814231cb540' THEN 'Early & Medieval Austria, to 1521' WHEN 'bokheim:history-lcc-full:52e23e99-8fd0-5429-9210-b090ef0d821a' THEN 'Margraviate & Duchy, 976–1246' WHEN 'unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9' THEN 'Habsburg Monarchy, 1521–1792' WHEN 'bokheim:history-lcc:9dcb1e55-2cad-5073-a488-3e73ff95c74e' THEN 'Reformation & Thirty Years’ War, 1521–1648' WHEN 'bokheim:history-lcc:67752989-ba16-5db3-a35e-96e53fd55bf6' THEN 'Absolutist Monarchy, 1648–1740' WHEN 'bokheim:history-lcc:94565cae-cefc-550a-87c5-a7513736999e' THEN 'Theresian & Josephine Era, 1740–1792' WHEN 'bokheim:history-lcc:ff83be8a-1c2c-5a81-b237-d5dc5d28d17b' THEN 'Revolutionary & Napoleonic Era, 1792–1815' WHEN 'bokheim:history-lcc:16d3ecc5-a8da-52a9-bcfa-b21609580d6b' THEN 'Revolution of 1848' WHEN 'bokheim:history-lcc:690bf9cb-ea63-5da0-8898-0dc7576cb6ec' THEN 'Anschluss, 1938–1945' WHEN 'bokheim:history-lcc:d06e62ee-6171-5292-ba6f-285209d4df8e' THEN 'Allied Occupation, 1945–1955' WHEN 'bokheim:history-lcc:d2e4a164-6678-5287-bc69-918460d02739' THEN 'Second Republic, 1955–Present' ELSE preferred_label END WHERE concept_id IN (SELECT concept_id FROM at_keep);
DROP TABLE at_edge; DROP TABLE at_shared; DROP TABLE at_keep; DROP TABLE at_tree;
COMMIT;
