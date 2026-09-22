-- Shorten the current longest curated display labels.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Malta, 1798–1802' WHERE concept_id='bokheim:history-lcc-full:ca56600d-8faf-5cdb-a588-61fc9e79675b';
UPDATE concept SET preferred_label='Malta Before 870' WHERE concept_id='bokheim:history-lcc-full:c7152445-32a5-5ab4-9b5d-5be085372459';
UPDATE concept SET preferred_label='Malta, 870–1530' WHERE concept_id='bokheim:history-lcc-full:71d19c58-4824-5624-afa3-10868b3839d6';
UPDATE concept SET preferred_label='Atrocities by Locality' WHERE concept_id='bokheim:history-lcc-full:ad045430-f508-5326-9f2f-f5f751fcdf71';
UPDATE concept SET preferred_label='Disinfection & Decontamination Methods' WHERE concept_id='bokheim:public-health:disinfection-fumigation-sterilization-decontamination';
UPDATE concept SET preferred_label='Operating Rooms, Equipment & Instruments' WHERE concept_id='unified:30a7dd35-22ea-51bb-aa33-9d3fabfb21bf';
UPDATE concept SET preferred_label='Plains, Plateaus, Steppes & Tundras' WHERE concept_id='bokheim:lcc-detail:af74957b-a409-53e6-a5ad-9f3e98c4ba30';
UPDATE concept SET preferred_label='Portugal & North Atlantic Islands' WHERE concept_id='bokheim:lcc-detail:d33b1bb8-6ee0-5f89-bd85-8e8b3dd94d0c';
UPDATE concept SET preferred_label='Systems Engineering & Operations Research' WHERE concept_id='bokheim:subject-bibliography:system-analysis-systems-engineering-operations-research';
UPDATE concept SET preferred_label='Accessible & Barrier-Free Design' WHERE preferred_label='Access for people with disabilities. Barrier-free design';
UPDATE concept SET preferred_label='Africa in Antiquity' WHERE concept_id='bokheim:history-lcc-full:ddf3c156-a762-547e-8ea4-33d6364d5643';
UPDATE concept SET preferred_label='Books for Special Audiences & Institutions' WHERE concept_id='unified:6e4b91d6-9a25-598f-9841-7b69d0c3d9e0';
UPDATE concept SET preferred_label='Bridge Construction, Maintenance & Repair' WHERE concept_id='bokheim:lcc-detail:f92e033d-0778-5a56-8b04-f55773874aed';
UPDATE concept SET preferred_label='Eastern Catholic Canon Law' WHERE concept_id='unified:822fa7c5-ebe5-5dc2-95ee-21fd7908317c';
UPDATE concept SET preferred_label='Woodcut & Wood-Engraving Collections' WHERE concept_id='bokheim:lcc-detail:11b29a38-aff6-534a-8199-953a0cadcd8d';
UPDATE concept SET preferred_label='National Diet, Food Supply & Nutrition Policy' WHERE concept_id='bokheim:lcc-detail:453baee6-85f5-5913-a91b-ea7213bea0a0';
UPDATE concept SET preferred_label='Temporomandibular Joint Disorders' WHERE concept_id='bokheim:lcc-detail:d450a1c3-34ea-58bc-ace5-8e19bcc42fff';
UPDATE concept SET preferred_label='International Genealogical Lists' WHERE concept_id='unified:0762efd2-de81-595e-89ec-a02c5fa26acb';
UPDATE concept SET preferred_label='Gold, Silver & Precious-Metal Mining' WHERE concept_id='bokheim:lcc-detail:1cf9596f-79fc-5aa7-95ad-623607f30e7b';
UPDATE concept SET preferred_label='Health Indicators, Statistics & Surveys' WHERE concept_id='bokheim:lcc-detail:77529317-acb1-53b0-90cd-5cba3195ca5c';
UPDATE concept SET preferred_label='Illuminated Works: Histories & Reproductions' WHERE concept_id='bokheim:lcc-detail:b13a72a7-cb80-5f49-969e-313bfa7d0ee1';
UPDATE concept SET preferred_label='History of Biography & Biographers' WHERE concept_id='unified:84665913-76dd-5dfc-babe-ebd9207b974a';
UPDATE concept SET preferred_label='Hypnotism, Animal Magnetism & Biomagnetism' WHERE concept_id='unified:f9d8f1e1-bf19-5447-b9a5-5e229b8f9d96';
UPDATE concept SET preferred_label='Intercultural Decorative Arts' WHERE concept_id='bokheim:lcc-detail:a15b06d6-d4cb-5193-8ed0-06f2f9534ea8';

COMMIT;
