PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Essays is a form subdivision; keep its BISAC coverage directly on the
-- History & Reference container instead of a selector-only leaf.
UPDATE source_selector
SET concept_id='bokheim:politics:history-reference', ordinal=1
WHERE concept_id='unified:be041117-49d1-5e11-9581-c484f5566f3e'
  AND system_id='bisac' AND selector='POL032000';
DELETE FROM concept WHERE concept_id='unified:be041117-49d1-5e11-9581-c484f5566f3e';

-- General IGO coverage plus the existing explicit League/UN selectors.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES ('unified:87887070-8a74-5ccf-bce6-fcf1bdfba9f9','lcc','JZ4850..JZ5490@3',0);
UPDATE source_selector
SET concept_id='unified:87887070-8a74-5ccf-bce6-fcf1bdfba9f9',
    ordinal=ordinal+1
WHERE concept_id='unified:9ed99353-c2a1-5b20-9628-0f16b5dcd127';
DELETE FROM concept WHERE concept_id='unified:9ed99353-c2a1-5b20-9628-0f16b5dcd127';

-- Transfer already-curated specific NGO and utopia selectors out of their
-- broad LCC parents so each code has one specific owner.
UPDATE source_selector
SET concept_id='unified:aa40bfc6-a495-5d2c-8aa3-513fbe4b6e61',
    ordinal=CASE selector
      WHEN 'JZ4841..JZ4848@3' THEN 0
      WHEN 'JZ4841..JZ4848@4' THEN 1
    END
WHERE concept_id='unified:dd047832-1545-59b2-8ee8-e71c48614c60'
  AND system_id='lcc'
  AND selector IN ('JZ4841..JZ4848@3','JZ4841..JZ4848@4');

UPDATE source_selector
SET concept_id='unified:ddb4088f-b10a-53e0-8f09-0351e2cc0a1f',
    ordinal=CASE selector
      WHEN 'HX806' THEN 0
      WHEN 'HX806..HX811@2' THEN 1
    END
WHERE concept_id='unified:56d80652-27bf-591d-b99f-54405b056e80'
  AND system_id='lcc'
  AND selector IN ('HX806','HX806..HX811@2');

-- Exact corruption heading in general public administration.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES ('unified:c4a2caab-d963-5ac0-a67a-ed94bbdeb6cf','lcc','JF1525.C66',0);

COMMIT;
