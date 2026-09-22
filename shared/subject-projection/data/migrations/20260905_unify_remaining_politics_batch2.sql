PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Unify general/compare constitutional law with the political Constitutions
-- subject. Country-jurisdiction constitutional selectors are not included.
WITH candidates AS (
 SELECT system_id,selector,
        row_number() OVER (PARTITION BY system_id ORDER BY ordinal,selector)-1 AS new_ordinal
 FROM source_selector
 WHERE concept_id='bokheim:law:comparative:constitutional'
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:d7d2d86b-50ab-506e-8f27-b291d4f7430b',system_id,selector,new_ordinal
FROM candidates;
DELETE FROM concept WHERE concept_id='bokheim:law:comparative:constitutional';

-- Extract general/comparative government from the over-combined political
-- institutions and public administration leaf.
WITH moved(selector,new_ordinal) AS (VALUES
 ('JF20..JF1177@2',0),
 ('JF51..JF56@3',1),
 ('JF51..JF56@4',2)
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:67b62f28-1b6f-5259-98e4-c86e58ad9607','lcc',selector,new_ordinal
FROM moved;
DELETE FROM source_selector
WHERE concept_id='unified:d559fba0-8aae-58dd-9df0-7e459aef7cb2'
  AND system_id='lcc'
  AND selector IN ('JF20..JF1177@2','JF51..JF56@3','JF51..JF56@4');

-- Dedicated arms-control and disarmament regimes from Law of nations.
WITH moved(selector,new_ordinal) AS (VALUES
 ('KZ5615..KZ5893@4',0),
 ('KZ5637..KZ5645@5',1),
 ('KZ5647..KZ5686@5',2),
 ('KZ5687..KZ5788.5@5',3),
 ('KZ5834..KZ5865@5',4),
 ('KZ5870..KZ5893@5',5)
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:7ebb91a9-c32b-5f60-a079-1fcdf6b2508f','lcc',selector,new_ordinal
FROM moved
WHERE EXISTS (
 SELECT 1 FROM source_selector s
 WHERE s.concept_id='unified:daf6b15e-d5bc-5e9b-887f-714ede8b99fd'
   AND s.system_id='lcc' AND s.selector=moved.selector
);
DELETE FROM source_selector
WHERE concept_id='unified:daf6b15e-d5bc-5e9b-887f-714ede8b99fd'
  AND system_id='lcc'
  AND selector IN ('KZ5615..KZ5893@4','KZ5637..KZ5645@5',
                   'KZ5647..KZ5686@5','KZ5687..KZ5788.5@5',
                   'KZ5834..KZ5865@5','KZ5870..KZ5893@5');

-- General treaty sources and the law/system of treaties.
WITH moved(selector,new_ordinal) AS (VALUES
 ('KZ118..KZ194@3',0),
 ('KZ118..KZ194@4',1),
 ('KZ119..KZ165@5',2),
 ('KZ170..KZ173@5',3),
 ('KZ176..KZ182.5@5',4),
 ('KZ183..KZ183.5@5',5),
 ('KZ184..KZ194@5',6),
 ('KZ1298..KZ1304@2',7)
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:e89bd13d-3519-538a-aacf-d4d41be57593','lcc',selector,new_ordinal
FROM moved
WHERE EXISTS (
 SELECT 1 FROM source_selector s
 WHERE s.concept_id='unified:daf6b15e-d5bc-5e9b-887f-714ede8b99fd'
   AND s.system_id='lcc' AND s.selector=moved.selector
);
DELETE FROM source_selector
WHERE concept_id='unified:daf6b15e-d5bc-5e9b-887f-714ede8b99fd'
  AND system_id='lcc'
  AND selector IN ('KZ118..KZ194@3','KZ118..KZ194@4',
                   'KZ119..KZ165@5','KZ170..KZ173@5',
                   'KZ176..KZ182.5@5','KZ183..KZ183.5@5',
                   'KZ184..KZ194@5','KZ1298..KZ1304@2');

COMMIT;
