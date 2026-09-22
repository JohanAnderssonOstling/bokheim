-- Separate chronological browsing from countries and topics in East Africa
-- and North Africa.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id, preferred_label) VALUES
 ('bokheim:history-lcc-full:f6adc36d-921d-5dcf-b7cf-a1b1d02fb729', 'By Period'),
 ('bokheim:history-lcc:63d5b47e-b1d0-5fc5-b9f8-84316d2351ce', 'By Period');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
 ('bokheim:history-lcc-full:f6adc36d-921d-5dcf-b7cf-a1b1d02fb729', 'unified:9e4616b4-77f2-5f15-b705-5eb06aedcb98', 0, 0),
 ('bokheim:history-lcc:63d5b47e-b1d0-5fc5-b9f8-84316d2351ce', 'unified:60bcae6e-928b-57c6-8c80-e2cf9d7a6246', 0, 0);

UPDATE concept_parent
SET parent_concept_id = 'bokheim:history-lcc-full:f6adc36d-921d-5dcf-b7cf-a1b1d02fb729',
    display_order = CASE concept_id
      WHEN 'bokheim:history-lcc-full:6e866a6b-2a6d-52c0-af83-2fddae32e8d7' THEN 0
      WHEN 'bokheim:history-lcc-full:7a1d4fc4-5db2-5798-afad-e6039084391b' THEN 10
      WHEN 'bokheim:history-lcc-full:ebcae9f4-695f-5087-9300-eebe6a7824fc' THEN 20
      WHEN 'bokheim:history-lcc-full:cef08502-f62f-5626-a8b7-b31c4710381d' THEN 30
      WHEN 'bokheim:history-lcc-full:fa2f0388-0f83-5e5f-9e6b-5374021b63c4' THEN 40
    END
WHERE parent_concept_id = 'unified:9e4616b4-77f2-5f15-b705-5eb06aedcb98'
  AND concept_id IN (
    'bokheim:history-lcc-full:6e866a6b-2a6d-52c0-af83-2fddae32e8d7',
    'bokheim:history-lcc-full:7a1d4fc4-5db2-5798-afad-e6039084391b',
    'bokheim:history-lcc-full:ebcae9f4-695f-5087-9300-eebe6a7824fc',
    'bokheim:history-lcc-full:cef08502-f62f-5626-a8b7-b31c4710381d',
    'bokheim:history-lcc-full:fa2f0388-0f83-5e5f-9e6b-5374021b63c4'
  );

UPDATE concept_parent
SET parent_concept_id = 'bokheim:history-lcc:63d5b47e-b1d0-5fc5-b9f8-84316d2351ce',
    display_order = CASE concept_id
      WHEN 'bokheim:history-lcc-full:de2a056a-779f-5b7d-a3da-d5dc58c5d026' THEN 0
      WHEN 'bokheim:history-lcc:7c1f136a-9d1b-583e-b87b-68dafb0ad5ad' THEN 10
      WHEN 'bokheim:history-lcc-full:0dba707f-d0a5-5d9a-8efe-2f9c0012ac2f' THEN 20
      WHEN 'bokheim:history-lcc-full:2ba8dffb-72c6-574f-b280-05a3530036f3' THEN 30
      WHEN 'bokheim:history-lcc-full:ca5d2ba6-5270-5b17-bd08-c0dfcd971835' THEN 40
      WHEN 'bokheim:history-lcc-full:443cddf8-6cf7-5cdb-91b1-081d2682477e' THEN 50
      WHEN 'bokheim:history-lcc-full:8d0fbcdb-5b95-51af-8a9d-d811c83e5cdd' THEN 60
      WHEN 'bokheim:history-lcc-full:17688740-1b7c-5fbd-a7b0-624dc30e0d0b' THEN 70
      WHEN 'bokheim:history-lcc-full:2520a48f-f429-5cbe-9b43-d90f9b42b7e7' THEN 80
      WHEN 'bokheim:history-lcc-full:fd4cee24-f997-5f80-9e1d-f81add35c15b' THEN 90
    END
WHERE parent_concept_id = 'unified:60bcae6e-928b-57c6-8c80-e2cf9d7a6246'
  AND concept_id IN (
    'bokheim:history-lcc-full:de2a056a-779f-5b7d-a3da-d5dc58c5d026',
    'bokheim:history-lcc:7c1f136a-9d1b-583e-b87b-68dafb0ad5ad',
    'bokheim:history-lcc-full:0dba707f-d0a5-5d9a-8efe-2f9c0012ac2f',
    'bokheim:history-lcc-full:2ba8dffb-72c6-574f-b280-05a3530036f3',
    'bokheim:history-lcc-full:ca5d2ba6-5270-5b17-bd08-c0dfcd971835',
    'bokheim:history-lcc-full:443cddf8-6cf7-5cdb-91b1-081d2682477e',
    'bokheim:history-lcc-full:8d0fbcdb-5b95-51af-8a9d-d811c83e5cdd',
    'bokheim:history-lcc-full:17688740-1b7c-5fbd-a7b0-624dc30e0d0b',
    'bokheim:history-lcc-full:2520a48f-f429-5cbe-9b43-d90f9b42b7e7',
    'bokheim:history-lcc-full:fd4cee24-f997-5f80-9e1d-f81add35c15b'
  );

COMMIT;
