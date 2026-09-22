BEGIN;
CREATE TEMP TABLE _country(label TEXT,parent_id INTEGER);
INSERT INTO _country VALUES ('Spanish Travel',2471),('Portuguese Travel',2471);
INSERT INTO concept(preferred_label) SELECT label FROM _country s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM _country s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);

CREATE TEMP TABLE _topic(label TEXT,parent_label TEXT,selector TEXT);
INSERT INTO _topic VALUES
 ('Balearic Islands Travel','Spanish Travel','DP302.B2'),('Balearic Islands Travel','Spanish Travel','DP302.B21'),
 ('Cádiz Travel','Spanish Travel','DP302.C2'),('Cádiz Travel','Spanish Travel','DP302.C21'),
 ('Canary Islands Travel','Spanish Travel','DP302.C385'),('Canary Islands Travel','Spanish Travel','DP302.C39'),
 ('Catalonia Travel','Spanish Travel','DP302.C6'),('Catalonia Travel','Spanish Travel','DP302.C61'),
 ('Galicia Travel','Spanish Travel','DP302.G14'),('Galicia Travel','Spanish Travel','DP302.G15'),
 ('Gibraltar Travel','Spanish Travel','DP302.G35'),('Gibraltar Travel','Spanish Travel','DP302.G36'),
 ('Cantabria Travel','Spanish Travel','DP302.S35'),('Seville Travel','Spanish Travel','DP302.S58'),
 ('Valencia Travel','Spanish Travel','DP302.V15'),('Valencia Travel','Spanish Travel','DP302.V16'),
 ('Biscay Travel','Spanish Travel','DP302.V44'),('Biscay Travel','Spanish Travel','DP302.V45'),
 ('Madrid Travel','Spanish Travel','DP354'),('Madrid Travel','Spanish Travel','DP355'),
 ('Barcelona Travel','Spanish Travel','DP402.B24'),('Barcelona Travel','Spanish Travel','DP402.B25'),
 ('Seville Travel','Spanish Travel','DP402.S38'),('Seville Travel','Spanish Travel','DP402.S4'),
 ('Toledo Travel','Spanish Travel','DP402.T7'),('Valencia Travel','Spanish Travel','DP402.V18'),
 ('Algarve Travel','Portuguese Travel','DP702.A28'),('Azores Travel','Portuguese Travel','DP702.A85'),
 ('Coimbra Travel','Portuguese Travel','DP702.C414'),('Portuguese Estremadura Travel','Portuguese Travel','DP702.E49'),
 ('Madeira Travel','Portuguese Travel','DP702.M15'),('Lisbon Travel','Portuguese Travel','DP757'),
 ('Piedmont & Savoy Travel','Italian Travel','DG612'),('Piedmont & Savoy Travel','Italian Travel','DG613'),
 ('Genoa Travel','Italian Travel','DG632'),('Milan & Lombardy Travel','Italian Travel','DG652'),
 ('Milan & Lombardy Travel','Italian Travel','DG653'),('Milan Travel','Italian Travel','DG659.52'),
 ('Venice Travel','Italian Travel','DG672'),('Tuscany & Florence Travel','Italian Travel','DG732'),
 ('Tuscany & Florence Travel','Italian Travel','DG733'),('Vatican & Papal States Travel','Italian Travel','DG792'),
 ('Vatican & Papal States Travel','Italian Travel','DG793'),('Vatican & Papal States Travel','Italian Travel','DG794'),
 ('Rome Travel','Italian Travel','DG804'),('Naples Travel','Italian Travel','DG842'),
 ('Naples Travel','Italian Travel','DG843'),('Sicily Travel','Italian Travel','DG862'),
 ('Sicily Travel','Italian Travel','DG863'),('Bologna Travel','Italian Travel','DG975.B58'),
 ('Brescia Travel','Italian Travel','DG975.B79'),('Sardinia Travel','Italian Travel','DG975.S3');

INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _topic s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,pc.concept_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label,parent_label FROM _topic) s
JOIN concept c ON c.preferred_label=s.label JOIN concept pc ON pc.preferred_label=s.parent_label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=pc.concept_id);

DELETE FROM lcc_selector WHERE concept_id IN (2471,2464) AND selector IN (SELECT selector FROM _topic);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _topic s JOIN concept c ON c.preferred_label=s.label;

-- Route country-wide and chronological travel directly to each country.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM lcc_selector s JOIN concept c ON c.preferred_label='Spanish Travel'
WHERE s.concept_id=2471 AND s.selector NOT GLOB 'DP[5-9]*';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM lcc_selector s JOIN concept c ON c.preferred_label='Portuguese Travel'
WHERE s.concept_id=2471 AND s.selector GLOB 'DP[5-9]*';
DELETE FROM lcc_selector WHERE concept_id=2471;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,v.selector FROM concept c JOIN (
 SELECT 'Spanish Travel' label,'DP14' selector UNION ALL SELECT 'Spanish Travel','DP27.5'
 UNION ALL SELECT 'Spanish Travel','DP28' UNION ALL SELECT 'Spanish Travel','DP32'
 UNION ALL SELECT 'Spanish Travel','DP34' UNION ALL SELECT 'Spanish Travel','DP38'
 UNION ALL SELECT 'Spanish Travel','DP41' UNION ALL SELECT 'Spanish Travel','DP42'
 UNION ALL SELECT 'Spanish Travel','DP43' UNION ALL SELECT 'Spanish Travel','DP43.2'
 UNION ALL SELECT 'Portuguese Travel','DP516' UNION ALL SELECT 'Portuguese Travel','DP521-526.5'
) v ON v.label=c.preferred_label;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (2464,'DG27-41'),(2464,'DG421.5-430.2');
DELETE FROM lcc_range WHERE concept_id IN (2471,2464);

DROP TABLE _topic; DROP TABLE _country;
COMMIT;
