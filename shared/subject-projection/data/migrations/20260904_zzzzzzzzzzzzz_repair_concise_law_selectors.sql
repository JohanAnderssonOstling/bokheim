-- Repair selector ownership after the detailed LCC law schedules were
-- collapsed into the concise curated Law hierarchy. A selector belongs to
-- its most specific retained subject; broader matches come from ancestry.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- KBR is the History of canon law schedule. Catholic Canon Law also has its
-- own BX selectors, so do not duplicate the KBR schedule onto that concept.
DELETE FROM source_selector
WHERE concept_id='unified:e8852e6c-2231-5163-8147-79489bb9b28f'
  AND selector LIKE 'KBR%';
DELETE FROM source_selector
WHERE concept_id='unified:f2803454-d4a0-5bad-af84-4e16242ff72a'
  AND selector LIKE 'KBU%';

-- This concept accidentally absorbed nearly the complete KJC European-law
-- schedule. Retain only the range that the outline actually calls
-- "Individual offenses"; its enclosing criminal-law ranges match by ancestry.
DELETE FROM source_selector
WHERE concept_id='unified:1bc3f93a-27b4-5ede-8a1a-24f48b2ded8a'
  AND selector LIKE 'KJC%'
  AND selector<>'KJC8356..KJC9380@3';
DELETE FROM source_selector
WHERE concept_id='unified:5c3ddf52-5dbf-5112-879e-8b436e90292e'
  AND selector='KJC8356..KJC9380@3';

-- Prefer a focused topical leaf over a broad or neighboring retained leaf.
CREATE TEMP TABLE law_selector_owner(
    winner TEXT NOT NULL,
    loser TEXT NOT NULL,
    PRIMARY KEY(winner, loser)
) WITHOUT ROWID;

INSERT INTO law_selector_owner VALUES
 ('bokheim:subject:law:business:ip-competition','bokheim:subject:law:regulation:technology-media'),
 ('bokheim:subject:law:business:transport','bokheim:subject:law:regulation:infrastructure'),
 ('bokheim:law:public:finance-property','bokheim:law:procedure:remedies-enforcement'),
 ('bokheim:law:profession:education-research','bokheim:subject:law:regulation:technology-media'),
 ('bokheim:subject:law:business:regulation','unified:b87a48b3-750a-5cfd-b079-9f760a875d24'),
 ('bokheim:subject:law:business:regulation','bokheim:law:public:policy-services'),
 ('bokheim:subject:law:business:regulation','bokheim:subject:law:regulation:infrastructure'),
 ('bokheim:subject:law:business:corporate','unified:5c3ddf52-5dbf-5112-879e-8b436e90292e'),
 ('bokheim:law:profession:organizations-publications','bokheim:law:public:administration'),
 ('bokheim:law:public:government-structure','bokheim:subject:law:regulation:other'),
 ('bokheim:subject:law:systems:customary','bokheim:law:public:policy-services'),
 ('bokheim:subject:law:regulation:social-welfare','unified:5c3ddf52-5dbf-5112-879e-8b436e90292e'),
 ('bokheim:subject:law:regulation:social-welfare','unified:120d8171-c5e5-54b9-a8d3-181886d2eb65'),
 ('bokheim:subject:law:business:transport','bokheim:law:public:policy-services'),
 ('bokheim:law:procedure:remedies-enforcement','unified:5c3ddf52-5dbf-5112-879e-8b436e90292e'),
 ('bokheim:law:procedure:remedies-enforcement','unified:120d8171-c5e5-54b9-a8d3-181886d2eb65'),
 ('bokheim:law:public:policy-services','unified:120d8171-c5e5-54b9-a8d3-181886d2eb65');

DELETE FROM source_selector AS losing
WHERE EXISTS (
    SELECT 1
    FROM law_selector_owner ownership
    JOIN source_selector winning
      ON winning.concept_id=ownership.winner
     AND winning.system_id=losing.system_id
     AND winning.selector=losing.selector
    WHERE ownership.loser=losing.concept_id
);

DROP TABLE law_selector_owner;

-- Religious-law schedules remain distinct from the general public-policy
-- grouping. European economic regulation remains under the topical leaf.
DELETE FROM source_selector AS policy
WHERE policy.concept_id='bokheim:law:public:policy-services'
  AND EXISTS (
      SELECT 1 FROM source_selector religious
      WHERE religious.concept_id='unified:7246f5ca-d9ac-5ed8-9ea6-e574b62447a2'
        AND religious.system_id=policy.system_id
        AND religious.selector=policy.selector
  )
  AND (policy.selector LIKE 'KB%' AND policy.selector NOT LIKE 'KJC%' AND policy.selector NOT LIKE 'KJE%');

DELETE FROM source_selector AS religious
WHERE religious.concept_id='unified:7246f5ca-d9ac-5ed8-9ea6-e574b62447a2'
  AND EXISTS (
      SELECT 1 FROM source_selector policy
      WHERE policy.concept_id='bokheim:law:public:policy-services'
        AND policy.system_id=religious.system_id
        AND policy.selector=religious.selector
  );

COMMIT;
