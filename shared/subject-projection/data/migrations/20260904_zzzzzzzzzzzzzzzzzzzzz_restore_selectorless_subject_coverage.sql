-- Restore precise LCC/BISAC coverage to curated leaves whose selectors had
-- been rolled into broad Catholic or Protestant parent concepts.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE restored_selector(
    target_id TEXT NOT NULL,
    system_id TEXT NOT NULL,
    selector TEXT NOT NULL,
    PRIMARY KEY(system_id,selector)
) WITHOUT ROWID;

INSERT INTO restored_selector VALUES
 ('bokheim:lcc-detail:9bb6c090-4bf6-5ebe-a686-35acabaeabfb','lcc','LB1044.875..LB1044.875@4'),
 ('bokheim:lcc-detail:dabee6ab-2b16-5dc2-85a9-228fa4927f18','lcc','TJ211.4152..TJ211.4152@4'),
 ('unified:436444bc-3202-5440-bfff-6d287b069437','bisac','REL093000'),
 ('unified:436444bc-3202-5440-bfff-6d287b069437','bisac','REL097000'),
 ('unified:436444bc-3202-5440-bfff-6d287b069437','bisac','REL111000'),
 ('unified:436444bc-3202-5440-bfff-6d287b069437','lcc','BX8901..BX9225@4'),
 ('unified:436444bc-3202-5440-bfff-6d287b069437','lcc','BX9884..BX9886@4'),

 ('bokheim:catholic:governance-clergy-administration','lcc','BX1800..BX1920@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1925..BX1925@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1927..BX1927@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1930..BX1932@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1933..BX1933@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1934..BX1934@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1934.582..BX1934.582@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1945..BX1945@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1947..BX1947@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1950..BX1950@4'),
 ('bokheim:catholic:governance-clergy-administration','lcc','BX1955..BX1955@4'),

 ('bokheim:catholic:institutions-traditions-biography','lcc','BX2400..BX4563@4'),
 ('bokheim:catholic:institutions-traditions-biography','lcc','BX4600..BX4644@4'),
 ('bokheim:catholic:institutions-traditions-biography','lcc','BX4650..BX4705@4'),
 ('bokheim:catholic:institutions-traditions-biography','lcc','BX4710..BX4715.95@4'),
 ('bokheim:catholic:institutions-traditions-biography','lcc','BX4716.4..BX4795@4'),

 ('bokheim:catholic:reference-history-study','lcc','BX800..BX806@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX808..BX816@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX817.A1..BX817.Z@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX818.A1..BX818.5@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX819..BX819.3@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX820..BX838@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX840.A1..BX840.Z@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX841..BX841@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX842..BX842@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX842.5..BX842.5@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX843..BX843@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX845..BX845@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX847..BX847@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX850..BX875@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX880..BX891@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX895..BX935@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX938..BX939@4'),
 ('bokheim:catholic:reference-history-study','lcc','BX940..BX1745@4'),

 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX1958..BX1968@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX1969..BX1969@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2177..BX2199.2@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2200..BX2292@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2295..BX2310@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2312..BX2312@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2315..BX2315@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2320..BX2321@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2323..BX2323.52@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2324..BX2324.2@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2325..BX2334.2@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2340..BX2340@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2345..BX2345@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2347..BX2348@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2349..BX2377@4'),
 ('bokheim:catholic:worship-devotion-religious-life','lcc','BX2380..BX2387.2@4');

DELETE FROM source_selector
WHERE (system_id,selector) IN (SELECT system_id,selector FROM restored_selector);

WITH numbered AS (
    SELECT target_id,system_id,selector,
           row_number() OVER (PARTITION BY target_id,system_id ORDER BY selector)-1 AS ordinal
    FROM restored_selector
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,ordinal FROM numbered;

DROP TABLE restored_selector;
COMMIT;
