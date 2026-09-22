BEGIN IMMEDIATE;

-- The old bucket mixed Russian/Soviet chronology with transnational local
-- history. Put the chronology under Russia and isolate the latter explicitly.
UPDATE concept SET preferred_label = 'General History & Soviet Era'
WHERE concept_id = 'bokheim:history:soviet-union-former-republics';

DELETE FROM concept_parent
WHERE concept_id = 'bokheim:history:soviet-union-former-republics';
INSERT INTO concept_parent (concept_id, parent_concept_id, ordinal, display_order)
VALUES ('bokheim:history:soviet-union-former-republics',
        'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10', 0, 10);

DELETE FROM source_selector
WHERE concept_id = 'bokheim:history:soviet-union-former-republics';
INSERT INTO source_selector (concept_id, system_id, selector, ordinal)
VALUES
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK15', 0),
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK18.6..DK18.65@1', 1),
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK18.7', 2),
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK18.8', 3),
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK30', 4),
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK32..DK35.5@1', 5),
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK36..DK293@1', 6),
  ('bokheim:history:soviet-union-former-republics', 'lcc', 'DK295', 7);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:history:former-soviet-regions', 'Former Soviet Regions'),
  ('bokheim:history:russia:general-regions', 'Russia — General & Regional History');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:history:former-soviet-regions',
   'unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec', 0, 65),
  ('bokheim:history:russia:general-regions',
   'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10', 0, 20);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK500', 0),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.A2..DK502.82@1', 1),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.3', 2),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.4', 3),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.42', 4),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.62..DK501.63@1', 5),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.65..DK501.66@1', 6),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.7..DK501.72@1', 7),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.75..DK501.76@1', 8),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.8..DK501.82@1', 9),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK501.92', 10),
  ('bokheim:history:former-soviet-regions', 'lcc', 'DK509', 11);

-- Russia is a container with geographic children, so it must not retain broad
-- permissive ranges of its own.
DELETE FROM source_selector
WHERE concept_id = 'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'
  AND system_id = 'lcc';

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:history:russia:general-regions', 'lcc', 'DK510..DK540.9@1', 0),
  ('bokheim:history:russia:general-regions', 'lcc', 'DK579.1..DK587.9@1', 1),
  ('bokheim:history:russia:general-regions', 'lcc', 'DK609.1..DK651@1', 2);

DELETE FROM source_selector
WHERE concept_id = 'bokheim:history:russia:siberia'
  AND system_id = 'lcc';
INSERT INTO source_selector (concept_id, system_id, selector, ordinal)
VALUES
  ('bokheim:history:russia:siberia', 'lcc', 'DK750', 0),
  ('bokheim:history:russia:siberia', 'lcc', 'DK751..DK781@1', 1);

COMMIT;
