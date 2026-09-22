-- Move remaining direct European regional periods into By period and merge
-- the duplicated Balkan 20th-century entry. Full source detail remains master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Merge the duplicate full-detail Balkan 20th-century concept into the
-- canonical period already inside By period.
INSERT INTO source_label(concept_id,label,ordinal)
SELECT 'bokheim:history-lcc:16ae8a9a-1837-591d-8ca7-18136343fb14',source.label,
       COALESCE((SELECT max(ordinal) FROM source_label
                 WHERE concept_id='bokheim:history-lcc:16ae8a9a-1837-591d-8ca7-18136343fb14'),-1)
       + row_number() OVER (ORDER BY source.ordinal,source.label)
FROM source_label source
WHERE source.concept_id='bokheim:history-lcc-full:3634300b-ea79-5d10-ac0d-a3e13b47b12c'
  AND NOT EXISTS (
      SELECT 1 FROM source_label existing
      WHERE existing.concept_id='bokheim:history-lcc:16ae8a9a-1837-591d-8ca7-18136343fb14'
        AND existing.label=source.label
  );

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:history-lcc:16ae8a9a-1837-591d-8ca7-18136343fb14',source.system_id,source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector
                 WHERE concept_id='bokheim:history-lcc:16ae8a9a-1837-591d-8ca7-18136343fb14'
                   AND system_id=source.system_id),-1)
       + row_number() OVER (PARTITION BY source.system_id ORDER BY source.ordinal,source.selector)
FROM source_selector source
WHERE source.concept_id='bokheim:history-lcc-full:3634300b-ea79-5d10-ac0d-a3e13b47b12c'
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id='bokheim:history-lcc:16ae8a9a-1837-591d-8ca7-18136343fb14'
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

DELETE FROM concept_parent
WHERE concept_id='bokheim:history-lcc-full:3634300b-ea79-5d10-ac0d-a3e13b47b12c'
   OR parent_concept_id='bokheim:history-lcc-full:3634300b-ea79-5d10-ac0d-a3e13b47b12c';
DELETE FROM concept
WHERE concept_id='bokheim:history-lcc-full:3634300b-ea79-5d10-ac0d-a3e13b47b12c';

CREATE TEMP TABLE region_period_move(
 concept_id TEXT PRIMARY KEY,
 region_id TEXT NOT NULL,
 wrapper_id TEXT NOT NULL
) WITHOUT ROWID;
INSERT INTO region_period_move VALUES
 ('unified:43849f41-4d9b-5ec8-99e3-cb5d0c448d88','unified:d6daa986-0fa4-5394-945e-614d7b09d3ca','unified:ea45ab61-b6a1-51e6-b034-3ee2ce84f3b1'),
 ('unified:50436c79-04f9-55ed-a357-a1134f40a3d2','unified:d6daa986-0fa4-5394-945e-614d7b09d3ca','unified:ea45ab61-b6a1-51e6-b034-3ee2ce84f3b1'),
 ('unified:0c30b071-ebf6-5ed2-b0cb-7a4d3976f5e1','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad','unified:3f539c15-67eb-5735-8dfb-819a1c1476fb'),
 ('unified:c29ac7ba-3323-5752-9c0f-2ed1dffe7c28','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad','unified:3f539c15-67eb-5735-8dfb-819a1c1476fb'),
 ('unified:a9658840-ef89-5168-9bcc-ad126ef55c49','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad','unified:3f539c15-67eb-5735-8dfb-819a1c1476fb'),
 ('unified:4479ebb6-eb6a-5fdc-8de2-1052217da3bc','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad','unified:3f539c15-67eb-5735-8dfb-819a1c1476fb'),
 ('unified:fe492861-7211-5d88-b443-ebf6220ad777','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad','unified:3f539c15-67eb-5735-8dfb-819a1c1476fb'),
 ('unified:0cd9d9d2-5957-56a4-ae99-2e5360709608','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad','unified:3f539c15-67eb-5735-8dfb-819a1c1476fb');

DELETE FROM concept_parent
WHERE (concept_id,parent_concept_id) IN (
 SELECT concept_id,region_id FROM region_period_move
);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT move.concept_id,move.wrapper_id,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent existing
                 WHERE existing.concept_id=move.concept_id),0),
       COALESCE((SELECT max(display_order)+10 FROM concept_parent existing
                 WHERE existing.parent_concept_id=move.wrapper_id),10)
       + row_number() OVER (PARTITION BY move.wrapper_id ORDER BY item.preferred_label)
FROM region_period_move move
JOIN concept item ON item.concept_id=move.concept_id;

DROP TABLE region_period_move;
COMMIT;
