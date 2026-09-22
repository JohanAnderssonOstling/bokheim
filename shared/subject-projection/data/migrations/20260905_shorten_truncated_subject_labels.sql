-- Replace source-truncated LCC headings with concise subject-facing labels.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Human–Alien Contact'
WHERE concept_id='unified:b4ba72fb-3eb0-577c-b6ae-0a28cc0cf036';

UPDATE concept SET preferred_label='Wartime Neutrality'
WHERE concept_id='unified:4de32799-353f-5cae-b23a-2bea790b449d';

UPDATE concept SET preferred_label='Colonization & Migration'
WHERE concept_id='unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b';

UPDATE concept SET preferred_label='Irrigation & Land Reclamation'
WHERE concept_id='unified:23375734-8734-5835-8fa5-b62f28c203ed';

UPDATE concept SET preferred_label='Hindi, Urdu & Hindustani'
WHERE concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47';

UPDATE concept SET preferred_label='School Facilities & Architecture'
WHERE concept_id='unified:d6be7c65-b2f5-589c-991b-42c6090bd471';

UPDATE concept SET preferred_label='Student Guidance & Counseling'
WHERE concept_id='bokheim:lcc-detail:1031a303-4f28-57dd-85f7-714b377b8334';

COMMIT;
