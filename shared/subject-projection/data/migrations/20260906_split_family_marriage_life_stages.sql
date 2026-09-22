BEGIN IMMEDIATE;

-- The parent is navigational. Remove its generated permissive HQ503-HQ1064
-- selector and let mutually exclusive leaves own the official LC divisions.
DELETE FROM source_selector
WHERE concept_id = 'unified:db804dff-8e30-5061-874c-e868a674f971'
  AND system_id = 'lcc';

UPDATE concept SET preferred_label = 'General Family Studies'
WHERE concept_id = 'bokheim:family-studies:history';

DELETE FROM source_selector WHERE concept_id IN (
  'bokheim:family-studies:history',
  'bokheim:family-studies:policy',
  'bokheim:family-studies:parenthood'
) AND system_id = 'lcc';

-- This imported cross-scheme bucket overlaps the official child-development
-- division. Keep its BISAC selectors but let the HQ span resolve here.
DELETE FROM source_selector
WHERE concept_id = 'bokheim:family:parenting-children'
  AND system_id = 'lcc';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:family:eugenics', 'Eugenics'),
  ('bokheim:family:size', 'Family Size'),
  ('bokheim:family:child-development', 'Children & Child Development'),
  ('bokheim:family:youth-adolescence', 'Youth & Adolescence'),
  ('bokheim:family:young-adults', 'Young Adults'),
  ('bokheim:family:adulthood', 'Adulthood'),
  ('bokheim:family:single-people', 'Single People'),
  ('bokheim:family:courtship-dating', 'Courtship & Dating'),
  ('bokheim:family:marriage-issues-services', 'Marriage Issues & Services'),
  ('bokheim:family:divorce', 'Divorce'),
  ('bokheim:family:free-love', 'Free Love'),
  ('bokheim:family:plural-marriage', 'Plural Marriage'),
  ('bokheim:family:unmarried-parenthood', 'Unmarried Parenthood'),
  ('bokheim:family:marriage-religion', 'Marriage & Religion'),
  ('bokheim:family:widowhood', 'Widowhood'),
  ('bokheim:family:aging', 'Aging & Gerontology');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:family:romantic-relationships', 'unified:db804dff-8e30-5061-874c-e868a674f971', 1, 15),
  ('bokheim:family:eugenics', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 30),
  ('bokheim:family:size', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 50),
  ('bokheim:family:child-development', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 60),
  ('bokheim:family:youth-adolescence', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 70),
  ('bokheim:family:young-adults', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 80),
  ('bokheim:family:adulthood', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 90),
  ('bokheim:family:single-people', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 100),
  ('bokheim:family:courtship-dating', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 110),
  ('bokheim:family:marriage-issues-services', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 120),
  ('bokheim:family:divorce', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 130),
  ('bokheim:family:free-love', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 140),
  ('bokheim:family:plural-marriage', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 150),
  ('bokheim:family:unmarried-parenthood', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 160),
  ('bokheim:family:marriage-religion', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 180),
  ('bokheim:family:widowhood', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 190),
  ('bokheim:family:aging', 'unified:db804dff-8e30-5061-874c-e868a674f971', 0, 200);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:family-studies:history', 'lcc', 'HQ503..HQ744.9@1', 0),
  ('bokheim:family-studies:history', 'lcc', 'HQ746..HQ749.9@1', 1),
  ('bokheim:family:eugenics', 'lcc', 'HQ750..HQ755.5@1', 0),
  ('bokheim:family-studies:parenthood', 'lcc', 'HQ755.7..HQ759.97@1', 0),
  ('bokheim:family:size', 'lcc', 'HQ760..HQ767.7@1', 0),
  ('bokheim:family:child-development', 'lcc', 'HQ767.8..HQ792.99@1', 0),
  ('bokheim:family:youth-adolescence', 'lcc', 'HQ793..HQ799.49@1', 0),
  ('bokheim:family:young-adults', 'lcc', 'HQ799.5..HQ799.9@1', 0),
  ('bokheim:family:adulthood', 'lcc', 'HQ799.95..HQ799.99@1', 0),
  ('bokheim:family:single-people', 'lcc', 'HQ800..HQ800.4@1', 0),
  ('bokheim:family:courtship-dating', 'lcc', 'HQ801..HQ801.83@1', 0),
  ('bokheim:family:marriage-issues-services', 'lcc', 'HQ802..HQ809.3@1', 0),
  ('bokheim:family:divorce', 'lcc', 'HQ811..HQ960.7@1', 0),
  ('bokheim:family:free-love', 'lcc', 'HQ961..HQ967@1', 0),
  ('bokheim:family:plural-marriage', 'lcc', 'HQ970..HQ997.99@1', 0),
  ('bokheim:family:unmarried-parenthood', 'lcc', 'HQ998..HQ999@1', 0),
  ('bokheim:family-studies:policy', 'lcc', 'HQ1001..HQ1006@1', 0),
  ('bokheim:family:marriage-religion', 'lcc', 'HQ1017..HQ1057@1', 0),
  ('bokheim:family:widowhood', 'lcc', 'HQ1058..HQ1058.5@1', 0),
  ('bokheim:family:aging', 'lcc', 'HQ1059..HQ1064@1', 0);

COMMIT;
