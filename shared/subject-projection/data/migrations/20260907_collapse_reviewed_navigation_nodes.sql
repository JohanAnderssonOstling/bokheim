-- Collapse approved navigation wrappers; preserve surviving IDs and selectors.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE collapse_guard(ok INTEGER CHECK(ok=1));
-- Collections & Miscellany -> American Literature
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5861 AND preferred_label='Collections & Miscellany';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5861;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=5861;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5861;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5861 AND parent_concept_id=3853 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3856 AND parent_concept_id=5861 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=3853 WHERE concept_id=3856 AND parent_concept_id=5861;
DELETE FROM concept WHERE concept_id=5861;
-- General, Theory & Institutions -> Arts & Media
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5592 AND preferred_label='General, Theory & Institutions';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5592;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=5592;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5592;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5592 AND parent_concept_id=510 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=11215 AND parent_concept_id=5592 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=510 WHERE concept_id=11215 AND parent_concept_id=5592;
DELETE FROM concept WHERE concept_id=5592;
-- Painting & Artistic Subjects -> Media, Techniques & Practices
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=855 AND preferred_label='Painting & Artistic Subjects';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=855;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=855;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=855;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=855 AND parent_concept_id=5669 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=1032 AND parent_concept_id=855 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5669 WHERE concept_id=1032 AND parent_concept_id=855;
DELETE FROM concept WHERE concept_id=855;
-- Languages of Oceania -> Oceanian & Austronesian Languages
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=3792 AND preferred_label='Languages of Oceania';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=3792;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=3792;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3792;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3792 AND parent_concept_id=3705 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6966 AND parent_concept_id=3792 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=3705 WHERE concept_id=6966 AND parent_concept_id=3792;
DELETE FROM concept WHERE concept_id=3792;
-- Film, Video & Digital Imaging -> Photography & Imaging
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5852 AND preferred_label='Film, Video & Digital Imaging';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5852;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=5852;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5852;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5852 AND parent_concept_id=915 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=600 AND parent_concept_id=5852 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=915 WHERE concept_id=600 AND parent_concept_id=5852;
DELETE FROM concept WHERE concept_id=5852;
-- Media & Communication -> Arts & Media
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=719 AND preferred_label='Media & Communication';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=719;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=719;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=719;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=719 AND parent_concept_id=510 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=600 AND parent_concept_id=719 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=510 WHERE concept_id=600 AND parent_concept_id=719;
DELETE FROM concept WHERE concept_id=719;
-- Philosophy of Nature & Science -> Philosophy
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5531 AND preferred_label='Philosophy of Nature & Science';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5531;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=5531;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5531;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5531 AND parent_concept_id=4487 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=4489 AND parent_concept_id=5531 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4487 WHERE concept_id=4489 AND parent_concept_id=5531;
DELETE FROM concept WHERE concept_id=5531;
-- Coding & Switching Theory -> Discrete Mathematics
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=13330 AND preferred_label='Coding & Switching Theory';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=13330;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=13330;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13330;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13330 AND parent_concept_id=4933 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53147 AND parent_concept_id=13330 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4933 WHERE concept_id=53147 AND parent_concept_id=13330;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53148 AND parent_concept_id=13330 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4933 WHERE concept_id=53148 AND parent_concept_id=13330;
DELETE FROM concept WHERE concept_id=13330;
-- Architectural Composition & Space -> Architectural Design & Drawing
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=8351 AND preferred_label='Architectural Composition & Space';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=8351;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=8351;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=8351;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=8351 AND parent_concept_id=571 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53162 AND parent_concept_id=8351 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=571 WHERE concept_id=53162 AND parent_concept_id=8351;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53163 AND parent_concept_id=8351 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=571 WHERE concept_id=53163 AND parent_concept_id=8351;
DELETE FROM concept WHERE concept_id=8351;
-- Data Structures & Abstract Data Types -> Programming
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=11697 AND preferred_label='Data Structures & Abstract Data Types';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=11697;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=11697;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=11697;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=11697 AND parent_concept_id=152 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53160 AND parent_concept_id=11697 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=152 WHERE concept_id=53160 AND parent_concept_id=11697;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53161 AND parent_concept_id=11697 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=152 WHERE concept_id=53161 AND parent_concept_id=11697;
DELETE FROM concept WHERE concept_id=11697;
-- Hypertext & Hypermedia -> Interactive Media & Multimedia
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=13158 AND preferred_label='Hypertext & Hypermedia';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=13158;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=13158;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13158;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13158 AND parent_concept_id=102 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53133 AND parent_concept_id=13158 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=102 WHERE concept_id=53133 AND parent_concept_id=13158;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53134 AND parent_concept_id=13158 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=102 WHERE concept_id=53134 AND parent_concept_id=13158;
DELETE FROM concept WHERE concept_id=13158;
-- Japanese Diplomatic History -> Japanese History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51025 AND preferred_label='Japanese Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51025;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51025;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51025;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51025 AND parent_concept_id=3498 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51035 AND parent_concept_id=51025 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3498 WHERE concept_id=51035 AND parent_concept_id=51025;
DELETE FROM concept WHERE concept_id=51025;
-- Canadian Diplomatic History -> Canadian History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51026 AND preferred_label='Canadian Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51026;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51026;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51026;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51026 AND parent_concept_id=3400 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51042 AND parent_concept_id=51026 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3400 WHERE concept_id=51042 AND parent_concept_id=51026;
DELETE FROM concept WHERE concept_id=51026;
-- Cuban Diplomatic History -> Cuban History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51027 AND preferred_label='Cuban Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51027;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51027;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51027;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51027 AND parent_concept_id=3380 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51038 AND parent_concept_id=51027 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3380 WHERE concept_id=51038 AND parent_concept_id=51027;
DELETE FROM concept WHERE concept_id=51027;
-- Iranian Diplomatic History -> Iranian History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51028 AND preferred_label='Iranian Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51028;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51028;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51028;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51028 AND parent_concept_id=3636 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51039 AND parent_concept_id=51028 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3636 WHERE concept_id=51039 AND parent_concept_id=51028;
DELETE FROM concept WHERE concept_id=51028;
-- Israeli Diplomatic History -> Israel & Palestine
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51044 AND preferred_label='Israeli Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51044;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51044;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51044;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51044 AND parent_concept_id=3638 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51047 AND parent_concept_id=51044 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3638 WHERE concept_id=51047 AND parent_concept_id=51044;
DELETE FROM concept WHERE concept_id=51044;
-- Mexican Diplomatic History -> Mexican History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51045 AND preferred_label='Mexican Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51045;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51045;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51045;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51045 AND parent_concept_id=3391 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51048 AND parent_concept_id=51045 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3391 WHERE concept_id=51048 AND parent_concept_id=51045;
DELETE FROM concept WHERE concept_id=51045;
-- Indian Diplomatic History -> Indian History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51046 AND preferred_label='Indian Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51046;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51046;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51046;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51046 AND parent_concept_id=3496 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51049 AND parent_concept_id=51046 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3496 WHERE concept_id=51049 AND parent_concept_id=51046;
DELETE FROM concept WHERE concept_id=51046;
-- Turkish Diplomatic History -> Turkish History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51050 AND preferred_label='Turkish Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51050;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51050;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51050;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51050 AND parent_concept_id=3551 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51054 AND parent_concept_id=51050 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3551 WHERE concept_id=51054 AND parent_concept_id=51050;
DELETE FROM concept WHERE concept_id=51050;
-- Iraqi Diplomatic History -> Iraqi History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51051 AND preferred_label='Iraqi Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51051;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51051;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51051;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51051 AND parent_concept_id=3637 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51055 AND parent_concept_id=51051 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3637 WHERE concept_id=51055 AND parent_concept_id=51051;
DELETE FROM concept WHERE concept_id=51051;
-- Vietnamese Diplomatic History -> Vietnamese History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51052 AND preferred_label='Vietnamese Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51052;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51052;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51052;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51052 AND parent_concept_id=3513 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51056 AND parent_concept_id=51052 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3513 WHERE concept_id=51056 AND parent_concept_id=51052;
DELETE FROM concept WHERE concept_id=51052;
-- Taiwanese Diplomatic History -> Taiwanese History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51053 AND preferred_label='Taiwanese Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51053;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51053;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51053;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51053 AND parent_concept_id=10699 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51057 AND parent_concept_id=51053 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=10699 WHERE concept_id=51057 AND parent_concept_id=51053;
DELETE FROM concept WHERE concept_id=51053;
-- Nicaraguan Diplomatic History -> Nicaraguan History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51059 AND preferred_label='Nicaraguan Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51059;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51059;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51059;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51059 AND parent_concept_id=3392 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51063 AND parent_concept_id=51059 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3392 WHERE concept_id=51063 AND parent_concept_id=51059;
DELETE FROM concept WHERE concept_id=51059;
-- South African Diplomatic Hist. -> South African History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51060 AND preferred_label='South African Diplomatic Hist.';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51060;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51060;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51060;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51060 AND parent_concept_id=3367 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51064 AND parent_concept_id=51060 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3367 WHERE concept_id=51064 AND parent_concept_id=51060;
DELETE FROM concept WHERE concept_id=51060;
-- Spanish Diplomatic History -> Spanish History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51061 AND preferred_label='Spanish Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51061;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51061;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51061;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51061 AND parent_concept_id=3613 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51065 AND parent_concept_id=51061 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3613 WHERE concept_id=51065 AND parent_concept_id=51061;
DELETE FROM concept WHERE concept_id=51061;
-- Italian Diplomatic History -> Italian History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51062 AND preferred_label='Italian Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51062;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51062;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51062;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51062 AND parent_concept_id=3607 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51066 AND parent_concept_id=51062 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3607 WHERE concept_id=51066 AND parent_concept_id=51062;
DELETE FROM concept WHERE concept_id=51062;
-- Panamanian Diplomatic History -> Panamanian History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51068 AND preferred_label='Panamanian Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51068;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51068;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51068;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51068 AND parent_concept_id=3393 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51072 AND parent_concept_id=51068 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3393 WHERE concept_id=51072 AND parent_concept_id=51068;
DELETE FROM concept WHERE concept_id=51068;
-- Australian Diplomatic History -> Australian History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51069 AND preferred_label='Australian Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51069;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51069;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51069;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51069 AND parent_concept_id=3677 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51073 AND parent_concept_id=51069 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3677 WHERE concept_id=51073 AND parent_concept_id=51069;
DELETE FROM concept WHERE concept_id=51069;
-- Brazilian Diplomatic History -> Brazilian History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51070 AND preferred_label='Brazilian Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51070;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51070;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51070;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51070 AND parent_concept_id=3427 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51074 AND parent_concept_id=51070 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3427 WHERE concept_id=51074 AND parent_concept_id=51070;
DELETE FROM concept WHERE concept_id=51070;
-- Pakistani Diplomatic History -> Pakistani History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51071 AND preferred_label='Pakistani Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51071;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51071;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51071;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51071 AND parent_concept_id=3505 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51075 AND parent_concept_id=51071 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3505 WHERE concept_id=51075 AND parent_concept_id=51071;
DELETE FROM concept WHERE concept_id=51071;
-- Afghan Diplomatic History -> Afghan History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51076 AND preferred_label='Afghan Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51076;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51076;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51076;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51076 AND parent_concept_id=3486 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51079 AND parent_concept_id=51076 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=3486 WHERE concept_id=51079 AND parent_concept_id=51076;
DELETE FROM concept WHERE concept_id=51076;
-- Argentine Diplomatic History -> Argentine History
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=51077 AND preferred_label='Argentine Diplomatic History';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=51077;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=51077;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51077;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51077 AND parent_concept_id=10499 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=51080 AND parent_concept_id=51077 AND ordinal=1;
UPDATE concept_parent SET parent_concept_id=10499 WHERE concept_id=51080 AND parent_concept_id=51077;
DELETE FROM concept WHERE concept_id=51077;
DROP TABLE collapse_guard;
COMMIT;
