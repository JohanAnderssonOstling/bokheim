-- Remove redundant European period umbrellas and isolated event leaves while
-- preserving every selector on the nearest useful retained period or country.
-- Concurrent polities, dynastic subperiods, and Italy's deep chronology stay.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_merge(
  remove_id TEXT PRIMARY KEY,
  retain_id TEXT NOT NULL
) STRICT;

INSERT INTO period_merge VALUES
  -- Generic aggregates duplicated by complete national period sequences.
  ('bokheim:history-lcc-full:cceaa606-c31c-52f0-8ad8-642314d1ab0a','bokheim:history-lcc-full:26912220-a2fa-548e-a7cb-521cd99cd7ea'),
  ('bokheim:history-lcc:fb0a2acc-adcb-5470-a260-0ecff30850b4','unified:0fbb9742-b8c8-52d7-8d1d-aa88c39ee3fc'),
  ('bokheim:history-lcc:2f2c090e-ec39-5918-beee-70f392bdfa72','unified:4a84d354-9db9-5152-9897-c3af9dbcfa57'),
  ('bokheim:history-lcc:2fa57651-4366-5a78-9d1a-b2fcb8a0c408','unified:4a84d354-9db9-5152-9897-c3af9dbcfa57'),
  ('bokheim:history-lcc:195156dc-69f2-5ccd-af7b-dde3cd950450','unified:550fd12f-93fc-55dc-8a89-eefb55a0ff09'),
  ('bokheim:history-lcc:73f3d277-af20-57bf-ace6-37ae8b73cb05','unified:bfa4fe6f-f7d3-5e20-982b-fd1a308642b5'),
  ('bokheim:history-lcc:c633b163-6520-5b4e-a3be-30dffcd1f206','unified:bfa4fe6f-f7d3-5e20-982b-fd1a308642b5'),
  ('bokheim:history-lcc-full:e5c5e46a-32d2-591d-8db1-8bbe209592f9','bokheim:history-lcc:7bb97488-d5a9-50c7-bf16-c0645d1761d4'),
  ('bokheim:history-lcc-full:709d3df4-c308-5848-9a9b-3f337c54cb38','unified:e280b450-322d-528d-afe3-f905673ed43b'),
  ('bokheim:history-lcc:77a3343b-82df-5728-9251-a00fca0473e8','unified:ea45ab61-b6a1-51e6-b034-3ee2ce84f3b1'),
  ('bokheim:history-lcc:714114d8-e233-5718-b2e1-600bc6aa24a9','unified:4ce7974f-9207-5786-8ea5-68f41923874c'),

  -- City periods accidentally promoted to Finland during locality flattening.
  ('bokheim:history-lcc-full:a9be403d-886c-50a4-8966-adfd60a0f52e','unified:550fd12f-93fc-55dc-8a89-eefb55a0ff09'),
  ('bokheim:history-lcc-full:287c9400-112d-5c64-842f-4a30351ea33f','unified:550fd12f-93fc-55dc-8a89-eefb55a0ff09'),
  ('bokheim:history-lcc-full:a6fc7f21-8d19-5a52-a4c9-dcba261dad07','unified:550fd12f-93fc-55dc-8a89-eefb55a0ff09'),

  -- Individual event leaves already contained by meaningful periods.
  ('bokheim:history-lcc-full:2fcc86ad-8914-5b98-9700-06b788ac43fe','bokheim:history-lcc-full:47543eaf-f561-59ea-8cf6-416a27927a81'),
  ('bokheim:history-lcc-full:fce52308-ec20-5fb5-b748-d26a0af14835','bokheim:history-lcc-full:47543eaf-f561-59ea-8cf6-416a27927a81'),
  ('bokheim:history-lcc-full:5aea6a5d-d12f-558b-8c79-de686680fccb','bokheim:history-lcc-full:47543eaf-f561-59ea-8cf6-416a27927a81'),
  ('bokheim:history-lcc-full:a7755ce7-28a6-5ea5-9f69-bf7ec5fdfdc3','bokheim:history-lcc-full:dc506f29-46eb-52dc-a051-900650711256'),
  ('bokheim:history-lcc-full:b42690e5-1e44-59ff-b965-6fda81cb565f','bokheim:history-lcc-full:1392f956-1225-5747-ad5f-c73d3ebcecca'),
  ('bokheim:history-lcc-full:2a212efb-aa8d-5b2b-9db9-a69521ab2f7b','bokheim:history-lcc-full:fe0c1b47-cc51-5867-8d53-936fcd9137a7'),
  ('bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c','unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143'),
  ('bokheim:history-lcc-full:d4f5587b-798d-59d1-8039-2aa54d1dc427','bokheim:history-lcc-full:27c18fdc-0e8c-5528-b7f3-1f44df9c0fab'),
  ('bokheim:history-lcc-full:db1897c7-5d81-50ed-9d3d-ecc2275e5094','bokheim:history-lcc-full:8070e8e6-b099-5dc0-a793-ab0a1f9606be'),
  ('bokheim:history-lcc-full:ddb024cf-57e9-5885-a153-536dab038be2','bokheim:history-lcc:a887861d-f17b-5554-be81-22838a2865a8');

-- Abort through a uniqueness violation if a reviewed removal unexpectedly
-- gained children after this migration was authored.
CREATE TEMP TABLE leaf_guard(value INTEGER CHECK(value=0)) STRICT;
INSERT INTO leaf_guard(value)
SELECT 1 FROM concept_parent WHERE parent_concept_id IN (SELECT remove_id FROM period_merge);

WITH missing AS (
  SELECT m.retain_id,s.system_id,s.selector
  FROM period_merge m
  JOIN source_selector s ON s.concept_id=m.remove_id
  WHERE NOT EXISTS (
    SELECT 1 FROM source_selector existing
    WHERE existing.concept_id=m.retain_id
      AND existing.system_id=s.system_id
      AND existing.selector=s.selector
  )
), ranked AS (
  SELECT retain_id,system_id,selector,
         row_number() OVER (PARTITION BY retain_id,system_id ORDER BY selector)-1 AS offset
  FROM missing
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT retain_id,system_id,selector,
       (SELECT coalesce(max(existing.ordinal)+1,0)
        FROM source_selector existing
        WHERE existing.concept_id=ranked.retain_id
          AND existing.system_id=ranked.system_id) + offset
FROM ranked;

DELETE FROM concept_parent WHERE concept_id IN (SELECT remove_id FROM period_merge);
DELETE FROM concept WHERE concept_id IN (SELECT remove_id FROM period_merge);

-- Restore accurate labels for the three retained Transylvanian periods that
-- were flattened to Romania in the earlier locality simplification.
UPDATE concept SET preferred_label='Early Transylvania, to 1526'
WHERE concept_id='bokheim:history-lcc-full:146b8081-0f4a-5980-857a-8e03ab334622';
UPDATE concept SET preferred_label='Transylvania, 1918–Present'
WHERE concept_id='bokheim:history-lcc-full:3bc0f520-ece3-5ea9-94b6-a17b0b11b2ef';

DROP TABLE leaf_guard;
DROP TABLE period_merge;
COMMIT;
