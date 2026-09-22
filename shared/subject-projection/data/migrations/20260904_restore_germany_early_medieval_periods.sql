-- Restore two meaningful early-medieval German subperiods as terminal leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
('bokheim:history-lcc:be694ee1-5360-5547-b7d1-719ca45f9a14','481–918'),
('bokheim:history-lcc:c7dea359-5daa-5d83-839e-abc72673451f','Empire and Papacy');

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:history-lcc:be694ee1-5360-5547-b7d1-719ca45f9a14','lcc','DD127..DD135@7',0),
('bokheim:history-lcc:c7dea359-5daa-5d83-839e-abc72673451f','lcc','DD126.5..DD126.5@7',0);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT c.concept_id,'unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87',0,
       CASE c.concept_id
           WHEN 'bokheim:history-lcc:be694ee1-5360-5547-b7d1-719ca45f9a14' THEN 5
           ELSE 15
       END
FROM concept c
WHERE c.concept_id IN (
    'bokheim:history-lcc:be694ee1-5360-5547-b7d1-719ca45f9a14',
    'bokheim:history-lcc:c7dea359-5daa-5d83-839e-abc72673451f'
)
AND NOT EXISTS (
    SELECT 1 FROM concept_parent p
    WHERE p.concept_id=c.concept_id
      AND p.parent_concept_id='unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87'
);

COMMIT;
