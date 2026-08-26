-- Shorten verbose visible labels beneath Social Sciences without changing
-- concept identity, source coverage, or hierarchy. The existing Anthropology
-- child becomes Anthropology Fields to avoid repeating its renamed parent.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label = 'Anthropology'
WHERE concept_id = 'unified:382fd3a0-46d4-5262-9f64-e3d975fbad31';
UPDATE concept SET preferred_label = 'Anthropology Fields'
WHERE concept_id = 'unified:d16025f5-02c1-5239-978a-08cdd004c1d5';
UPDATE concept SET preferred_label = 'Military Studies'
WHERE concept_id = 'unified:6c61eaa2-34ac-5f77-a297-5988a5fc029f';
UPDATE concept SET preferred_label = 'Politics & Government'
WHERE concept_id = 'unified:fa671db5-b0ef-5492-9c50-638c60fa5148';
UPDATE concept SET preferred_label = 'Social Welfare & Services'
WHERE concept_id = 'unified:3cf48471-040f-5902-98c1-b29effbc984e';
UPDATE concept SET preferred_label = 'Sociology'
WHERE concept_id = 'unified:1dc4999e-4875-54c4-be21-e8b23a259be2';

UPDATE concept SET preferred_label = 'Accounting & Tax'
WHERE concept_id = 'bokheim:subject:economics:accounting-tax';
UPDATE concept SET preferred_label = 'Economic Policy'
WHERE concept_id = 'bokheim:subject:economics:policy-development';
UPDATE concept SET preferred_label = 'Work & Careers'
WHERE concept_id = 'bokheim:subject:economics:workplace';
UPDATE concept SET preferred_label = 'Trade & Transportation'
WHERE concept_id = 'bokheim:subject:economics:trade';
UPDATE concept SET preferred_label = 'Marketing & Sales'
WHERE concept_id = 'bokheim:subject:economics:marketing';
UPDATE concept SET preferred_label = 'Operations & Logistics'
WHERE concept_id = 'bokheim:subject:economics:operations';
UPDATE concept SET preferred_label = 'Governance & Ethics'
WHERE concept_id = 'bokheim:subject:economics:governance';
UPDATE concept SET preferred_label = 'Business Skills & Reference'
WHERE concept_id = 'bokheim:subject:economics:skills-reference';

UPDATE concept SET preferred_label = 'Government & Administration'
WHERE concept_id = 'bokheim:politics:government-administration';
UPDATE concept SET preferred_label = 'History & Reference'
WHERE concept_id = 'bokheim:politics:history-reference';
UPDATE concept SET preferred_label = 'International Relations'
WHERE concept_id = 'bokheim:politics:international-global';
UPDATE concept SET preferred_label = 'Public Policy & Civil Society'
WHERE concept_id = 'bokheim:politics:policy-economy';
UPDATE concept SET preferred_label = 'Politics, Media & Opinion'
WHERE concept_id = 'bokheim:politics:process-media';
UPDATE concept SET preferred_label = 'Political Theory'
WHERE concept_id = 'bokheim:politics:theory-ideologies';
UPDATE concept SET preferred_label = 'Rights & Citizenship'
WHERE concept_id = 'bokheim:politics:rights-citizenship';
UPDATE concept SET preferred_label = 'Security & Conflict'
WHERE concept_id = 'bokheim:politics:security-conflict';

UPDATE concept SET preferred_label = 'Identity & Community'
WHERE concept_id = 'bokheim:sociology:identity-community';
UPDATE concept SET preferred_label = 'Population & Life Course'
WHERE concept_id = 'bokheim:sociology:population-services';
UPDATE concept SET preferred_label = 'Social Change & Development'
WHERE concept_id = 'bokheim:sociology:change-development';
UPDATE concept SET preferred_label = 'Technology & Everyday Life'
WHERE concept_id = 'bokheim:sociology:technology-everyday';
UPDATE concept SET preferred_label = 'Theory & Research'
WHERE concept_id = 'bokheim:sociology:theory-research';

UPDATE concept SET preferred_label = 'Behaviorism'
WHERE concept_id = 'bokheim:philosophy-lcc-full:4ab60950-98d1-5c6e-bb45-991790a1c6cf';
UPDATE concept SET preferred_label = 'Clinical Psychology'
WHERE concept_id = 'bokheim:psychology:clinical-applied';
UPDATE concept SET preferred_label = 'Psychometrics'
WHERE concept_id = 'bokheim:philosophy-lcc-full:906ef9db-31ff-56e5-a80f-6473f0eec960';
UPDATE concept SET preferred_label = 'Motor Learning'
WHERE concept_id = 'bokheim:philosophy-lcc-full:eaa95b28-249e-535f-9ce1-e32dc1172df7';
UPDATE concept SET preferred_label = 'Existential & Phenomenological Psychology'
WHERE concept_id = 'bokheim:philosophy-lcc-full:2a1bcfe1-c4a7-5e30-9339-d1c3a3449677';
UPDATE concept SET preferred_label = 'Psychology of Religion'
WHERE concept_id = 'bokheim:philosophy-lcc-full:676d073f-65c4-58df-8430-0c9f1efb0519';
UPDATE concept SET preferred_label = 'Research & Assessment'
WHERE concept_id = 'bokheim:psychology:research-reference';
UPDATE concept SET preferred_label = 'Gestalt Psychology'
WHERE concept_id = 'bokheim:philosophy-lcc-full:822e1bad-0114-5ec4-8ec7-280d07937ad4';

UPDATE concept SET preferred_label = 'Criminal Justice'
WHERE concept_id = 'unified:9ee41e3f-1c87-5341-b1da-ac3c08891d55';
UPDATE concept SET preferred_label = 'Criminal Investigation'
WHERE concept_id = 'unified:be910a98-26d7-5955-9f56-a64ee0481691';
UPDATE concept SET preferred_label = 'Prisons & Corrections'
WHERE concept_id = 'unified:31ac004b-5b5b-5512-8008-542452648faf';
UPDATE concept SET preferred_label = 'Policing Methods'
WHERE concept_id = 'unified:58009fd0-0e0a-5549-bd8d-3a84fab2ca5c';
UPDATE concept SET preferred_label = 'Police & Detectives'
WHERE concept_id = 'unified:fa5ca91b-aa32-5d01-9ef9-e5153aed98ab';
UPDATE concept SET preferred_label = 'Social Welfare'
WHERE concept_id = 'bokheim:subject:social-issues:public-welfare';
UPDATE concept SET preferred_label = 'Secret Societies'
WHERE concept_id = 'unified:2989b21c-bc12-5f51-be30-a941d7b37948';

COMMIT;
