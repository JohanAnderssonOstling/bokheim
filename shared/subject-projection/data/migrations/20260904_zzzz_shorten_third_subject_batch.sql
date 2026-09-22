-- Short curated display labels for the third longest-name batch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Modern Literary Influence' WHERE concept_id='bokheim:lcc-detail:31dffa26-966b-5e17-bff6-6af11c549f82';
UPDATE concept SET preferred_label='Gardening & Landscape Design' WHERE concept_id='bokheim:subject-bibliography:gardening-horticulture-floriculture-landscape-gardening-and-architecture';
UPDATE concept SET preferred_label='Oratory Collections' WHERE concept_id='bokheim:lcc-detail:d30487f2-5d66-5900-a95b-df4df2bf6074';
UPDATE concept SET preferred_label='Skin Allergies' WHERE concept_id='bokheim:lcc-detail:b1e4cf7c-721f-5a88-a5b6-986fefa22378';
UPDATE concept SET preferred_label='School Meals & Clothing' WHERE concept_id='bokheim:lcc-detail:7c4b805b-a71c-53b3-aad5-0ff13c999a96';
UPDATE concept SET preferred_label='Pollution Measurement' WHERE concept_id='bokheim:lcc-detail:d1dc12f5-fcc8-5256-8f0b-f920beefb4a3';
UPDATE concept SET preferred_label='National & State Parks' WHERE concept_id='bokheim:history-lcc-full:2278a7bc-96b1-5081-81ec-3f5b233a5d1e';
UPDATE concept SET preferred_label='Home Food Preservation' WHERE concept_id='bokheim:lcc-detail:49e2b582-58b0-5e75-8f60-e1fa1a37dea4';
UPDATE concept SET preferred_label='Fluid & Pneumatic Conveying' WHERE concept_id='bokheim:lcc-detail:ac0bffc0-5d7c-5053-a701-31fe6aec65a3';
UPDATE concept SET preferred_label='Libraries & Society' WHERE concept_id='bokheim:library-science:libraries-and-community-libraries-and-society-social-aspects-of-libraries';
UPDATE concept SET preferred_label='Local & Industrial Railways' WHERE concept_id='bokheim:lcc-detail:67b6e834-1723-52fb-81c9-53a246bf3779';
UPDATE concept SET preferred_label='Pharmacognosy & Natural Drugs' WHERE concept_id='bokheim:lcc-detail:17712ac0-dda6-53a2-94dd-e1ffe9edcf48';
UPDATE concept SET preferred_label='Electric Machine Drives' WHERE concept_id='bokheim:lcc-detail:ccdc1e43-ac03-5e57-a3ff-1df40611f084';
UPDATE concept SET preferred_label='Foreign-Language Translations' WHERE concept_id='bokheim:lcc-detail:8830cf4a-3b1c-5fd0-9e84-c4fb76a436a8';
UPDATE concept SET preferred_label='Owner-Bound Sheet Music' WHERE concept_id='bokheim:lcc-detail:47a1e5b3-af18-5333-8ae1-c3af468f314b';
UPDATE concept SET preferred_label='Terminology & Symbols' WHERE concept_id='bokheim:lcc-detail:38b4342f-73b9-52a8-9725-4a9b588e8eb1';
UPDATE concept SET preferred_label='Copyrighted Works & Formats' WHERE concept_id='bokheim:lcc-detail:309b0c2e-dea7-54f1-9b88-82ac8f8336a3';
UPDATE concept SET preferred_label='Banned & Challenged Books' WHERE concept_id='bokheim:lcc-detail:aa699e69-54b1-541f-99c5-c0572a2afcde';
UPDATE concept SET preferred_label='Continuous Transformation Groups' WHERE concept_id='bokheim:lcc-detail:de458a5c-c032-50cc-9c4d-82cf151967ae';
UPDATE concept SET preferred_label='Endodontics' WHERE concept_id='bokheim:lcc-detail:4f8d120c-0469-5fa2-a1f2-11b98b777450';

COMMIT;
