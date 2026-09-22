-- Split Asian regional religion coverage while retaining the deliberately broad
-- fallback on an explicit leaf. Narrower country ranges win by priority.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:asian-religions:other-regions','General & Other Asian Religions'),
 ('bokheim:asian-religions:china','Religion in China'),
 ('bokheim:asian-religions:india','Religion in India'),
 ('bokheim:asian-religions:japan-korea','Religion in Japan & Korea');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:asian-religions:other-regions','bokheim:religion:asian-regional',0,100),
 ('bokheim:asian-religions:china','bokheim:religion:asian-regional',0,200),
 ('bokheim:asian-religions:india','bokheim:religion:asian-regional',0,300),
 ('bokheim:asian-religions:japan-korea','bokheim:religion:asian-regional',0,400);
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=0 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1030..BL1033@5';
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=1 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1750..BL2350@5';
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=2 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1050';
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=3 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1055';
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=4 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1060';
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=5 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1710';
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=6 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1000..BL1099.99@2';
UPDATE source_selector SET concept_id='bokheim:asian-religions:other-regions',ordinal=7 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1700..BL2389.99@2';
UPDATE source_selector SET concept_id='bokheim:asian-religions:china',ordinal=0 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1790..BL1975@6';
UPDATE source_selector SET concept_id='bokheim:asian-religions:china',ordinal=1 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1830..BL1883@7';
UPDATE source_selector SET concept_id='bokheim:asian-religions:china',ordinal=2 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL1899..BL1942.85@7';
UPDATE source_selector SET concept_id='bokheim:asian-religions:india',ordinal=0 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL2000..BL2032@6';
UPDATE source_selector SET concept_id='bokheim:asian-religions:india',ordinal=1 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL2017..BL2018.7@7';
UPDATE source_selector SET concept_id='bokheim:asian-religions:japan-korea',ordinal=0 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL2195..BL2228@6';
UPDATE source_selector SET concept_id='bokheim:asian-religions:japan-korea',ordinal=1 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL2216..BL2227.8@7';
UPDATE source_selector SET concept_id='bokheim:asian-religions:japan-korea',ordinal=2 WHERE concept_id='bokheim:religion:asian-regional' AND selector='BL2230..BL2240@6';
COMMIT;
