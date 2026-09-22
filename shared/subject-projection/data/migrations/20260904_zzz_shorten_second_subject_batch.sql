-- Short curated display labels for the second longest-name batch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Pacific Politics & Colonies' WHERE concept_id='bokheim:lcc-detail:aac3e4d9-f71b-5ec9-a52b-4b816551e4fa';
UPDATE concept SET preferred_label='Physical Medicine Practice' WHERE concept_id='bokheim:lcc-detail:bdf65bc4-a1f8-5d8e-8d5b-6e4a2be10655';
UPDATE concept SET preferred_label='Musician Registration & Standards' WHERE concept_id='bokheim:lcc-detail:e0ad89fa-b66f-512a-bc34-4afd27ecb321';
UPDATE concept SET preferred_label='Professional Authorship' WHERE concept_id='bokheim:lcc-detail:1e0b7917-2221-5495-8c30-23209306447a';
UPDATE concept SET preferred_label='Middle School Education' WHERE concept_id='bokheim:lcc-detail:95eab629-6d2b-5ed9-87e0-eda0a4eeb775';
UPDATE concept SET preferred_label='Clinician–Family Relationships' WHERE concept_id='bokheim:lcc-detail:b6debc2f-001a-5a40-9509-b66bce7cab16';
UPDATE concept SET preferred_label='End-of-Life Care & Euthanasia' WHERE concept_id='bokheim:lcc-detail:a8804101-8de6-59a8-8c66-18140cc2a664';
UPDATE concept SET preferred_label='Ethnic & Religious Folk Art' WHERE concept_id='bokheim:lcc-detail:dc6d5f5a-593a-5774-96d1-0faeaaa5cf85';
UPDATE concept SET preferred_label='Sustainable School Buildings' WHERE concept_id='bokheim:lcc-detail:ec27f707-f0cc-5128-a77f-5f5bed86903b';
UPDATE concept SET preferred_label='High-Risk Anesthesia' WHERE concept_id='bokheim:lcc-detail:bc8b82e4-7a5a-5db3-9b23-b6229dc4fd10';
UPDATE concept SET preferred_label='Muslims in Medieval Europe' WHERE concept_id='bokheim:history-lcc:def74869-39eb-596a-84b3-539db778178c';
UPDATE concept SET preferred_label='Geometric & Conformal Analysis' WHERE concept_id='bokheim:lcc-detail:8769420a-b8d2-5cbf-8977-5c73203f3cbb';
UPDATE concept SET preferred_label='Student Life & Conduct' WHERE concept_id='bokheim:lcc-detail:d301a97b-156a-5e1c-b1bc-0c7c3a4dc49a';
UPDATE concept SET preferred_label='Spanish Literature Studies' WHERE concept_id='bokheim:lcc-detail:9fdc1f21-e1c5-5bd4-afce-3a8f6549e9c2';
UPDATE concept SET preferred_label='Watercolor Subjects' WHERE concept_id='bokheim:lcc-detail:c53af47f-6568-547a-9fbe-162f6858c949';
UPDATE concept SET preferred_label='Italian Literature Studies' WHERE concept_id='bokheim:lcc-detail:dbfe4b30-4bc3-5e31-848d-2c1add323990';
UPDATE concept SET preferred_label='ANOVA & Experimental Design' WHERE concept_id='bokheim:lcc-detail:948803cf-72fa-53ec-ba0f-df04e033b20d';
UPDATE concept SET preferred_label='Family & Pregnancy Forensics' WHERE concept_id='bokheim:lcc-detail:db686aad-9a1b-58c4-84e8-25c5d39dd07f';
UPDATE concept SET preferred_label='Broadcast Distance Education' WHERE concept_id='bokheim:lcc-detail:e4b553a8-1f7a-536c-8df7-9c236fb1cbef';
UPDATE concept SET preferred_label='Holocaust Remembrance Day' WHERE concept_id='bokheim:history-lcc-full:bd516b2b-9fbc-540b-b8a6-fcd24c1d58d7';

COMMIT;
