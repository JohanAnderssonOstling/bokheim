-- Split the broad JN Europe leaf into mutually exclusive jurisdiction leaves.
-- Official boundaries: Library of Congress Classification, subclass JN.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES
 ('bokheim:politics:europe:general', 'Europe — General'),
 ('bokheim:politics:europe:eu', 'European Union & Integration'),
 ('bokheim:politics:europe:great-britain', 'Great Britain'),
 ('bokheim:politics:europe:ireland', 'Ireland'),
 ('bokheim:politics:europe:austria', 'Austria'),
 ('bokheim:politics:europe:hungary', 'Hungary'),
 ('bokheim:politics:europe:czech-republic', 'Czech Republic'),
 ('bokheim:politics:europe:slovakia', 'Slovakia'),
 ('bokheim:politics:europe:france', 'France'),
 ('bokheim:politics:europe:germany', 'Germany'),
 ('bokheim:politics:europe:greece', 'Greece'),
 ('bokheim:politics:europe:italy', 'Italy'),
 ('bokheim:politics:europe:netherlands', 'Netherlands'),
 ('bokheim:politics:europe:belgium', 'Belgium'),
 ('bokheim:politics:europe:soviet-union', 'Soviet Union & Former Republics'),
 ('bokheim:politics:europe:estonia', 'Estonia'),
 ('bokheim:politics:europe:ukraine', 'Ukraine'),
 ('bokheim:politics:europe:belarus', 'Belarus'),
 ('bokheim:politics:europe:moldova', 'Moldova'),
 ('bokheim:politics:europe:russia', 'Russia'),
 ('bokheim:politics:europe:latvia', 'Latvia'),
 ('bokheim:politics:europe:lithuania', 'Lithuania'),
 ('bokheim:politics:europe:poland', 'Poland'),
 ('bokheim:politics:europe:northern-europe', 'Northern Europe — General'),
 ('bokheim:politics:europe:denmark', 'Denmark'),
 ('bokheim:politics:europe:greenland', 'Greenland'),
 ('bokheim:politics:europe:iceland', 'Iceland'),
 ('bokheim:politics:europe:finland', 'Finland'),
 ('bokheim:politics:europe:norway', 'Norway'),
 ('bokheim:politics:europe:sweden', 'Sweden'),
 ('bokheim:politics:europe:spain', 'Spain'),
 ('bokheim:politics:europe:portugal', 'Portugal'),
 ('bokheim:politics:europe:switzerland', 'Switzerland'),
 ('bokheim:politics:europe:balkans', 'Balkan States — General'),
 ('bokheim:politics:europe:albania', 'Albania'),
 ('bokheim:politics:europe:bulgaria', 'Bulgaria'),
 ('bokheim:politics:europe:romania', 'Romania'),
 ('bokheim:politics:europe:yugoslavia', 'Yugoslavia'),
 ('bokheim:politics:europe:other-balkan', 'Other Balkan States');

WITH v(id,ord) AS (VALUES
 ('bokheim:politics:europe:general',0), ('bokheim:politics:europe:eu',1),
 ('bokheim:politics:europe:great-britain',2), ('bokheim:politics:europe:ireland',3),
 ('bokheim:politics:europe:austria',4), ('bokheim:politics:europe:hungary',5),
 ('bokheim:politics:europe:czech-republic',6), ('bokheim:politics:europe:slovakia',7),
 ('bokheim:politics:europe:france',8), ('bokheim:politics:europe:germany',9),
 ('bokheim:politics:europe:greece',10), ('bokheim:politics:europe:italy',11),
 ('bokheim:politics:europe:netherlands',12), ('bokheim:politics:europe:belgium',13),
 ('bokheim:politics:europe:soviet-union',14), ('bokheim:politics:europe:estonia',15),
 ('bokheim:politics:europe:ukraine',16), ('bokheim:politics:europe:belarus',17),
 ('bokheim:politics:europe:moldova',18), ('bokheim:politics:europe:russia',19),
 ('bokheim:politics:europe:latvia',20), ('bokheim:politics:europe:lithuania',21),
 ('bokheim:politics:europe:poland',22), ('bokheim:politics:europe:northern-europe',23),
 ('bokheim:politics:europe:denmark',24), ('bokheim:politics:europe:greenland',25),
 ('bokheim:politics:europe:iceland',26), ('bokheim:politics:europe:finland',27),
 ('bokheim:politics:europe:norway',28), ('bokheim:politics:europe:sweden',29),
 ('bokheim:politics:europe:spain',30), ('bokheim:politics:europe:portugal',31),
 ('bokheim:politics:europe:switzerland',32), ('bokheim:politics:europe:balkans',33),
 ('bokheim:politics:europe:albania',34), ('bokheim:politics:europe:bulgaria',35),
 ('bokheim:politics:europe:romania',36), ('bokheim:politics:europe:yugoslavia',37),
 ('bokheim:politics:europe:other-balkan',38)
)
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT v.id,'unified:8f2c2533-1a08-56ff-a786-52e091c4a82b',v.ord,v.ord
FROM v;

-- The all-JN range overlaps every jurisdiction and cannot remain once split.
DELETE FROM source_selector
WHERE concept_id='unified:8f2c2533-1a08-56ff-a786-52e091c4a82b'
  AND selector IN ('JN1..JN9689@1','JN1..JN97@2');

-- The Austria-Hungary range crosses the modern Austria/Hungary boundary.
DELETE FROM source_selector
WHERE concept_id='unified:8f2c2533-1a08-56ff-a786-52e091c4a82b'
  AND selector='JN1601..JN2191@2';
INSERT OR IGNORE INTO source_selector VALUES
 ('bokheim:politics:europe:austria','lcc','JN1601..JN2049@2',0),
 ('bokheim:politics:europe:hungary','lcc','JN2050..JN2191@2',0);

-- Move every remaining JN selector according to its lower classification number.
UPDATE source_selector
SET concept_id = CASE
 WHEN selector='JN' THEN 'bokheim:politics:europe:general'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 26 AND 50 THEN 'bokheim:politics:europe:eu'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 1 AND 97 THEN 'bokheim:politics:europe:general'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 101 AND 1371.99 THEN 'bokheim:politics:europe:great-britain'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 1395 AND 1571.99 THEN 'bokheim:politics:europe:ireland'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 1601 AND 2049.99 THEN 'bokheim:politics:europe:austria'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 2050 AND 2191.99 THEN 'bokheim:politics:europe:hungary'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 2210 AND 2229.99 THEN 'bokheim:politics:europe:czech-republic'
 WHEN CAST(substr(selector,3) AS REAL)=2240 THEN 'bokheim:politics:europe:slovakia'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 2301 AND 3007.99 THEN 'bokheim:politics:europe:france'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 3201 AND 4999.99 THEN 'bokheim:politics:europe:germany'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 5001 AND 5199.99 THEN 'bokheim:politics:europe:greece'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 5201 AND 5699.99 THEN 'bokheim:politics:europe:italy'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 5701 AND 5999.99 THEN 'bokheim:politics:europe:netherlands'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6101 AND 6379.99 THEN 'bokheim:politics:europe:belgium'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6500 AND 6599.99 THEN 'bokheim:politics:europe:soviet-union'
 WHEN CAST(substr(selector,3) AS REAL)=6615 THEN 'bokheim:politics:europe:estonia'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6630 AND 6639.99 THEN 'bokheim:politics:europe:ukraine'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6640 AND 6649.99 THEN 'bokheim:politics:europe:belarus'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6680 AND 6689.99 THEN 'bokheim:politics:europe:moldova'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6690 AND 6699.99 THEN 'bokheim:politics:europe:russia'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6730 AND 6739.99 THEN 'bokheim:politics:europe:latvia'
 WHEN CAST(substr(selector,3) AS REAL)=6745 THEN 'bokheim:politics:europe:lithuania'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 6750 AND 6769.99 THEN 'bokheim:politics:europe:poland'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 7011 AND 7066.99 THEN 'bokheim:politics:europe:northern-europe'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 7101 AND 7369.99 THEN 'bokheim:politics:europe:denmark'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 7370 AND 7379.99 THEN 'bokheim:politics:europe:greenland'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 7380 AND 7389.99 THEN 'bokheim:politics:europe:iceland'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 7390 AND 7399.99 THEN 'bokheim:politics:europe:finland'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 7401 AND 7699.99 THEN 'bokheim:politics:europe:norway'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 7721 AND 7999.99 THEN 'bokheim:politics:europe:sweden'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 8101 AND 8399.99 THEN 'bokheim:politics:europe:spain'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 8423 AND 8669.99 THEN 'bokheim:politics:europe:portugal'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 8701 AND 9599.99 THEN 'bokheim:politics:europe:switzerland'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 9600 AND 9609.99 THEN 'bokheim:politics:europe:balkans'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 9610 AND 9619.99 THEN 'bokheim:politics:europe:albania'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 9620 AND 9639.99 THEN 'bokheim:politics:europe:bulgaria'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 9640 AND 9659.99 THEN 'bokheim:politics:europe:romania'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 9660 AND 9679.99 THEN 'bokheim:politics:europe:yugoslavia'
 WHEN CAST(substr(selector,3) AS REAL) BETWEEN 9680 AND 9689.99 THEN 'bokheim:politics:europe:other-balkan'
 ELSE concept_id END
WHERE concept_id='unified:8f2c2533-1a08-56ff-a786-52e091c4a82b'
  AND system_id='lcc';

-- JN1-97 contains both general European material and the EU schedule. Split
-- the former broad range so the three resulting leaves do not overlap.
INSERT OR IGNORE INTO source_selector VALUES
 ('bokheim:politics:europe:general','lcc','JN1..JN25@2',10000),
 ('bokheim:politics:europe:eu','lcc','JN26..JN50@2',10000),
 ('bokheim:politics:europe:general','lcc','JN51..JN97@2',10001),
 -- Legacy/cutter values immediately following the published Ireland boundary.
 ('bokheim:politics:europe:ireland','lcc','JN1571.51..JN1599@2',10000),
 -- Older Czechoslovak cutter values precede the current JN2210 boundary.
 ('bokheim:politics:europe:czech-republic','lcc','JN2200..JN2209.99@2',10000);

-- Re-number selector ordinals independently within each new concept.
DROP TABLE IF EXISTS temp.resequenced_europe_selectors;
CREATE TEMP TABLE resequenced_europe_selectors AS
SELECT concept_id,system_id,selector,
       row_number() OVER (PARTITION BY concept_id,system_id ORDER BY ordinal,selector)-1 AS ordinal
FROM source_selector
WHERE concept_id LIKE 'bokheim:politics:europe:%';
DELETE FROM source_selector WHERE concept_id LIKE 'bokheim:politics:europe:%';
INSERT INTO source_selector SELECT concept_id,system_id,selector,ordinal
FROM resequenced_europe_selectors;

COMMIT;
