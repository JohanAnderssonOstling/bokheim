-- Apply curated display labels after all restoration migrations.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Atomic & Molecular Structure' WHERE concept_id='bokheim:lcc-detail:1951db22-ee37-5fa4-8b6c-6fab63f6b79c';
UPDATE concept SET preferred_label='Hospital Medical Management' WHERE concept_id='bokheim:lcc-detail:08f00646-6d0e-5188-932a-6fdd1e88c071';
UPDATE concept SET preferred_label='Minority & Bilingual Education' WHERE concept_id='bokheim:lcc-detail:e42d1337-16d3-5218-be73-e90c1da15991';
UPDATE concept SET preferred_label='Biotech Compounds' WHERE concept_id='bokheim:lcc-detail:25cc440a-6baf-5289-9886-8fbeffc82ec4';
UPDATE concept SET preferred_label='Yugoslavia, 1918–1945' WHERE concept_id='bokheim:history-lcc-full:02ac7177-3b6a-5df8-9400-6b603cc2b00b';
UPDATE concept SET preferred_label='Placental & Gestational Disorders' WHERE concept_id='bokheim:lcc-detail:3cbdc8b8-c4a3-5aee-8c75-ab8b459d9f12';
UPDATE concept SET preferred_label='Magnetic Compass Deviation' WHERE concept_id='bokheim:lcc-detail:61247679-df63-5351-9fb0-9792668ed393';
UPDATE concept SET preferred_label='UN Refugee Organizations' WHERE concept_id='bokheim:lcc-detail:ce1456ae-e3a1-50c3-909b-b3b3a1bc70d9';
UPDATE concept SET preferred_label='Medical Practice Regulation' WHERE concept_id='bokheim:lcc-detail:c8aded4e-1c1a-5c4e-800b-e182dcd6e883';
UPDATE concept SET preferred_label='Difference & Integral Equations' WHERE concept_id='bokheim:lcc-detail:6ea4b05b-d4c0-5684-b173-e6789f15602f';
UPDATE concept SET preferred_label='American Music Before 1860' WHERE concept_id='bokheim:lcc-detail:b4684045-f503-5f72-a6ae-f7d6c2e7fd01';
UPDATE concept SET preferred_label='Operating Room Assistance' WHERE concept_id='bokheim:lcc-detail:44b0106e-ae72-5bf1-b628-98b9fb3749c9';
UPDATE concept SET preferred_label='Art in Wartime' WHERE concept_id='bokheim:lcc-detail:e55dcf4f-8983-5c6c-899d-698c932051c4';
UPDATE concept SET preferred_label='Industrial Engineering Profession' WHERE concept_id='bokheim:lcc-detail:44f4ec8d-ae5e-544c-9aee-c3bc78d4c0d8';
UPDATE concept SET preferred_label='Electronics Manufacturing' WHERE concept_id='bokheim:lcc-detail:deed514d-6aa4-5ded-9f7b-f1feb02341e4';

COMMIT;
