-- Replace catalog-schedule phrasing with concise browse labels. This changes
-- labels only; concept identities, hierarchy, and selectors remain untouched.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Bulgars'
WHERE concept_id='bokheim:history-lcc-full:7b918f85-7357-5951-9b22-e64da8c80f37';
UPDATE concept SET preferred_label='Cimmerians'
WHERE concept_id='bokheim:history-lcc-full:66b4b9d4-5009-5221-8626-c960344a09b6';
UPDATE concept SET preferred_label='Scythians'
WHERE concept_id='bokheim:history-lcc-full:7c71b1af-83d3-58f4-8d67-f5e61f15ef8a';

UPDATE concept SET preferred_label='Latvia, 20th Century'
WHERE concept_id='bokheim:history-lcc-full:cceaa606-c31c-52f0-8ad8-642314d1ab0a';
UPDATE concept SET preferred_label='Polish People’s Republic, 1945–1989'
WHERE concept_id='bokheim:history-lcc:1bbadec6-7bfd-5b7e-80c4-ce1ac09cb331';
UPDATE concept SET preferred_label='Third Polish Republic, 1989–Present'
WHERE concept_id='bokheim:history-lcc:ac8b706d-47d8-57a9-ae5e-2a46658936db';
UPDATE concept SET preferred_label='Elective Monarchy, 1572–1763'
WHERE concept_id='bokheim:history-lcc-full:1eb9955d-7cba-5ab5-be79-162778819b25';
UPDATE concept SET preferred_label='Partition Era, 1763–1795'
WHERE concept_id='bokheim:history-lcc-full:5e4c164c-b331-5728-9133-c9ea71dfbfd8';
UPDATE concept SET preferred_label='Polish–Lithuanian Rule, 1340–1569'
WHERE concept_id='bokheim:history-lcc-full:f8fcae5a-01af-5855-8a7c-3c1cc7a1cbb4';
UPDATE concept SET preferred_label='Polish Rule, 1569–1648'
WHERE concept_id='bokheim:history-lcc-full:e7dc7e54-1298-53c7-8c00-920dd8c21534';

UPDATE concept SET preferred_label='Ottoman Decline, 1566–1640'
WHERE concept_id='unified:43849f41-4d9b-5ec8-99e3-cb5d0c448d88';
UPDATE concept SET preferred_label='Balkan Peninsula, 20th Century'
WHERE concept_id='bokheim:history-lcc:16ae8a9a-1837-591d-8ca7-18136343fb14';
UPDATE concept SET preferred_label='Kingdom of Romania, 1866/1881–1944'
WHERE concept_id='bokheim:history-lcc:5728520c-d064-56b2-9aff-cc09e851391f';
UPDATE concept SET preferred_label='Ottoman Thrace, 1362–1913'
WHERE concept_id='bokheim:history-lcc-full:747d1761-b2cb-586d-9307-c7046b535a9e';
UPDATE concept SET preferred_label='Ottoman Decline, 1566–1640'
WHERE concept_id='bokheim:history-lcc:d29dc224-5ca3-56ae-a39c-18e9e04abc8a';
UPDATE concept SET preferred_label='Tanzimat Era, 1839–1876'
WHERE concept_id='bokheim:history-lcc-full:091da0c5-5007-566f-8189-e6343d379825';

UPDATE concept SET preferred_label='Union with Sweden, 1814'
WHERE concept_id='bokheim:history-lcc:a4c6247e-c0f4-510c-bf1e-4f443e995ece';
UPDATE concept SET preferred_label='Late Roman Empire, 284–476'
WHERE concept_id='bokheim:history-lcc-full:f9d9aa2c-4d8c-55a6-b691-7f742b0628e3';
UPDATE concept SET preferred_label='Contemporary Italy, 1975–Present'
WHERE concept_id='bokheim:lcc-detail:89001e57-5d0a-5e23-9df9-d7fc89fe0ef7';

UPDATE concept SET preferred_label='Knights of Malta, 1530–1798'
WHERE concept_id='bokheim:history-lcc-full:dfda020c-f155-5d76-a9d2-88662e231a32';
UPDATE concept SET preferred_label='British Malta, 1798–1964'
WHERE concept_id='bokheim:history-lcc-full:709d3df4-c308-5848-9a9b-3f337c54cb38';
UPDATE concept SET preferred_label='British Administration, 1802–1947'
WHERE concept_id='bokheim:history-lcc-full:985f2659-c2cc-5995-8d37-b2714b1bf6ef';
UPDATE concept SET preferred_label='Maltese Self-Government, 1947–1964'
WHERE concept_id='bokheim:history-lcc-full:01aa8254-794b-55ea-80ea-10e2647a161d';
UPDATE concept SET preferred_label='Independent Malta, 1964–Present'
WHERE concept_id='bokheim:history-lcc-full:1bc8d040-ae0b-5f36-bab9-f0da6074fdb5';

UPDATE concept SET preferred_label='Duchy of Luxembourg, 1354–1815'
WHERE concept_id='bokheim:history-lcc-full:f9c4395a-a1fd-5ec7-83b0-92cdabc44197';
UPDATE concept SET preferred_label='Grand Duchy of Luxembourg, 1815–Present'
WHERE concept_id='bokheim:history-lcc-full:8070e8e6-b099-5dc0-a793-ab0a1f9606be';
UPDATE concept SET preferred_label='Dutch Republic, 1555–1795'
WHERE concept_id='bokheim:history-lcc:816749e3-2bf8-5fd2-bd47-264ac98e75ef';
UPDATE concept SET preferred_label='Second Bulgarian Empire & Asen Dynasty, 1185–1396'
WHERE concept_id='bokheim:history-lcc:3a76dde2-bbf0-51cc-8d3e-01ddf9083f36';

COMMIT;
