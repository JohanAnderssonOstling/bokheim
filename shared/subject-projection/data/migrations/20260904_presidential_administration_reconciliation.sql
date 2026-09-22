-- Presidential administrations are a temporal subdivision of United States
-- history. Consolidate them beneath Historical Eras and remove the parallel
-- top-level "By presidential administration" hierarchy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE administration_era (
    administration_id TEXT PRIMARY KEY,
    era_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO administration_era VALUES
    ('unified:e8c923b0-68e0-5fc3-a366-b35cb6f684c9', 'unified:be6be5ce-b6ae-506b-80a3-e3971e5866cb'),
    ('unified:fa5c1e14-43d5-5798-aea8-a676a5a3f41c', 'unified:be6be5ce-b6ae-506b-80a3-e3971e5866cb'),
    ('unified:b7b970e7-9cd5-560a-aa28-1da6fa85f24e', 'unified:0f229f96-a87f-5dc8-a3c5-04a9ca94f668'),
    ('unified:27b22b36-4cb3-5961-82da-a0c5141327d0', 'unified:0f229f96-a87f-5dc8-a3c5-04a9ca94f668'),
    ('unified:1318b880-e341-5652-9661-fc4b9b2c4b45', 'unified:0f229f96-a87f-5dc8-a3c5-04a9ca94f668'),
    ('unified:87a98b7f-2df9-5eb2-8be5-0c2ebb309669', 'unified:0f229f96-a87f-5dc8-a3c5-04a9ca94f668'),
    ('unified:fe4c30e3-0d2b-54bb-95da-0766ef5ffbae', 'unified:33a55847-ffd2-5470-a636-58599f22cbe4'),
    ('unified:fa3687fb-99b0-5754-81eb-e5b5c803f04f', 'unified:33a55847-ffd2-5470-a636-58599f22cbe4'),
    ('unified:c9fe87ca-c0d4-5fc2-82c8-874d358fd45e', 'unified:33a55847-ffd2-5470-a636-58599f22cbe4'),
    ('bokheim:lcc-detail:ab33be21-9c81-5432-96db-d9041b7e8bd6', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('bokheim:lcc-detail:b41cdd51-a266-577e-83ea-40aa71b48952', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('bokheim:lcc-detail:ea0fee00-71e4-510a-bf5c-31805bc535d7', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('bokheim:lcc-detail:b3b50535-9688-5d1b-8084-bf8600a7ebf8', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('bokheim:lcc-detail:4c240d6e-cbf2-5f81-9807-023f7c59ece3', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('bokheim:lcc-detail:ccd9c178-839e-55b7-9ef7-97780f6843e9', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('bokheim:lcc-detail:0f4bc72f-97ce-5136-bfeb-68017ebab2a8', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('unified:84301cf4-071b-5b62-a3a7-31f0178ed1d3', 'unified:671e3882-ff72-5b46-9055-e091c881d099'),
    ('bokheim:lcc-detail:22490cdd-11b9-5d20-9c92-ee043540f2aa', 'unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('bokheim:lcc-detail:da809d38-43f7-5cac-ae87-0acccfd1abd1', 'unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('bokheim:lcc-detail:69e32b1f-c8d3-5fba-b5be-f6f85d3d8364', 'unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('unified:44d62036-bca9-5332-8a9a-76b9cd93ee0b', 'unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('bokheim:lcc-detail:6c12c0fa-d371-5996-bdfd-fcf5804f5f8a', 'unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('bokheim:lcc-detail:0bffb5e1-413a-55e7-94cf-06c5a3c111ee', 'unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('bokheim:lcc-detail:1f44a1d1-653d-5efe-bd20-06b2780834b2', 'unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('unified:9bd2b54c-d150-589f-b5a2-fca48f6ec1fc', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('bokheim:lcc-detail:de60eede-b857-56f2-a05f-8c4534420e38', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('unified:5adac15f-e1a6-5ecd-bb73-97fb4d288efa', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('bokheim:lcc-detail:a6719c80-f10d-5172-aff3-f02c3794d4a6', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('bokheim:lcc-detail:1ff6b867-23e4-5781-91a1-7851b7492af0', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('unified:a2e69725-23f9-5d44-94b4-5857399890a1', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('bokheim:lcc-detail:339b712a-9df6-5c4c-9e17-aee80b8e0610', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('bokheim:lcc-detail:f3ff5f06-ac66-59ef-ab70-75bca8b21f68', 'unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('unified:461987d3-c50d-573e-abc2-57c309acd110', 'unified:deae860a-f708-5ec3-a9ee-5cac8c52dae4'),
    ('unified:18493c17-3bb8-5768-875a-605b6705b9ea', 'unified:deae860a-f708-5ec3-a9ee-5cac8c52dae4');

CREATE TEMP TABLE administration_merge (
    source_id TEXT PRIMARY KEY,
    canonical_id TEXT NOT NULL,
    era_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO administration_merge
SELECT mapping.administration_id,
       COALESCE((
           SELECT min(existing.concept_id)
           FROM concept_parent era_child
           JOIN concept existing ON existing.concept_id = era_child.concept_id
           WHERE era_child.parent_concept_id = mapping.era_id
             AND lower(existing.preferred_label) = lower(source.preferred_label)
       ), mapping.administration_id),
       mapping.era_id
FROM administration_era mapping
JOIN concept source ON source.concept_id = mapping.administration_id;

-- Preserve selectors and source labels from duplicate concepts.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal)
SELECT source.canonical_id, selector.system_id, selector.selector,
       COALESCE((SELECT max(existing.ordinal) FROM source_selector existing
                 WHERE existing.concept_id=source.canonical_id AND existing.system_id=selector.system_id), -1)
       + row_number() OVER (PARTITION BY source.canonical_id, selector.system_id ORDER BY selector.ordinal)
FROM administration_merge source
JOIN source_selector selector ON selector.concept_id = source.source_id
WHERE source.source_id <> source.canonical_id
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id=source.canonical_id
        AND existing.system_id=selector.system_id
        AND existing.selector=selector.selector
  );

INSERT INTO source_label (concept_id, label, ordinal)
SELECT source.canonical_id, label.label,
       COALESCE((SELECT max(existing.ordinal) FROM source_label existing
                 WHERE existing.concept_id=source.canonical_id), -1)
       + row_number() OVER (PARTITION BY source.canonical_id ORDER BY label.ordinal)
FROM administration_merge source
JOIN source_label label ON label.concept_id = source.source_id
WHERE source.source_id <> source.canonical_id
  AND NOT EXISTS (
      SELECT 1 FROM source_label existing
      WHERE existing.concept_id=source.canonical_id AND existing.label=label.label
  );

-- Reparent subordinate campaigns and topics to the canonical administration.
DELETE FROM concept_parent
WHERE EXISTS (
    SELECT 1 FROM administration_merge merge
    WHERE merge.source_id <> merge.canonical_id
      AND concept_parent.parent_concept_id = merge.source_id
      AND EXISTS (
          SELECT 1 FROM concept_parent existing
          WHERE existing.concept_id = concept_parent.concept_id
            AND existing.parent_concept_id = merge.canonical_id
      )
);

UPDATE concept_parent
SET parent_concept_id = (
    SELECT canonical_id FROM administration_merge
    WHERE source_id = concept_parent.parent_concept_id
)
WHERE parent_concept_id IN (
    SELECT source_id FROM administration_merge WHERE source_id <> canonical_id
);

-- Unique administrations move to their era. Existing canonical concepts only
-- lose the redundant parent edge.
UPDATE concept_parent
SET parent_concept_id = (
    SELECT era_id FROM administration_merge
    WHERE source_id = concept_parent.concept_id
)
WHERE parent_concept_id = (
        SELECT concept_id FROM concept WHERE preferred_label='By presidential administration'
      )
  AND concept_id IN (
      SELECT source_id FROM administration_merge WHERE source_id=canonical_id
  )
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      JOIN administration_merge merge ON merge.source_id=concept_parent.concept_id
      WHERE existing.concept_id=concept_parent.concept_id
        AND existing.parent_concept_id=merge.era_id
  );

DELETE FROM concept_parent
WHERE parent_concept_id = (
        SELECT concept_id FROM concept WHERE preferred_label='By presidential administration'
      )
  AND concept_id IN (SELECT canonical_id FROM administration_merge);

DELETE FROM concept_parent
WHERE concept_id IN (
    SELECT source_id FROM administration_merge WHERE source_id <> canonical_id
);

DELETE FROM concept
WHERE concept_id IN (
    SELECT source_id FROM administration_merge WHERE source_id <> canonical_id
);

DELETE FROM concept_parent
WHERE concept_id = (
    SELECT concept_id FROM concept WHERE preferred_label='By presidential administration'
);

DELETE FROM concept
WHERE preferred_label='By presidential administration'
  AND concept_id NOT IN (SELECT parent_concept_id FROM concept_parent);

DROP TABLE administration_merge;
DROP TABLE administration_era;

COMMIT;
