-- Consolidate Christianity and restore a selective BISAC/LCC-backed third
-- level suitable for browsing. DDC-only and publication-form concepts remain
-- excluded.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

-- Juvenile and young-adult Christianity are facets of the same concept, not
-- separate sibling concepts.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
    ('unified:2c4b22db-f725-5427-a930-210f2de6642c','bisac','YAN047040',1),
    ('unified:2c4b22db-f725-5427-a930-210f2de6642c','bisac','JNF049080',2);

DELETE FROM concept WHERE concept_id IN (
    'unified:72873374-67e0-5ea8-bcb0-9379bf48d113',
    'unified:e4e15557-684a-5864-b1c6-c85ade0d2ebb'
);

CREATE TEMP TABLE restore_christianity_concept(concept_id TEXT PRIMARY KEY);
INSERT INTO restore_christianity_concept VALUES
    -- Protestant traditions
    ('unified:c14aeb25-15f7-5c86-a4fd-db1836faf42b'),
    ('unified:88dc9a38-69a3-5019-9086-820b28c6dd96'),
    ('unified:bf1172da-5b27-5cca-97df-13c2de7071ed'),
    ('unified:5fb2d52c-b4b8-5719-ba39-9e40dec359ce'),
    ('unified:436444bc-3202-5440-bfff-6d287b069437'),
    ('unified:bb612d78-008d-546f-abf0-d9a2757dbe3d'),
    ('unified:25c4a755-9c54-5eb9-8e03-c14b77f4767d'),
    -- Catholic browse groups
    ('bokheim:catholic:doctrine-preaching-relations'),
    ('bokheim:catholic:governance-clergy-administration'),
    ('bokheim:catholic:institutions-traditions-biography'),
    ('bokheim:catholic:reference-history-study'),
    ('bokheim:catholic:worship-devotion-religious-life'),
    -- History and controversy
    ('unified:2b9f1e4f-6e2a-5a82-bb9d-a8b5cae7d709'),
    ('unified:b09cca90-854f-5cb4-8092-ea267c378cf7'),
    ('unified:f50ee53a-1bf6-5b98-95ea-11de72240d96'),
    ('unified:ff7cd087-3d6b-594d-bc27-b992f4f321b9'),
    ('unified:893c8e74-cc06-5182-a777-6a012295fdb0'),
    ('unified:cb4a0a0e-5bcb-5f66-ac96-4edf163deb60'),
    -- Theology
    ('unified:3c31a4c9-f232-50b1-b2f8-c69b1bdf83f3'),
    ('unified:2ecc5011-f950-50dc-9983-de9370ca7b79'),
    ('unified:da2b82e3-992d-5056-b5ed-99a183801264'),
    ('unified:74db0a97-60c6-56bd-bc7c-27498015d201'),
    ('unified:ab206421-921d-5a90-a4b2-a5e216c5e902'),
    ('unified:b6791316-4d0d-5fee-a8a4-e4cc4335dd1e'),
    ('unified:f12a125b-fe42-5ce1-8318-2e4595c3c066'),
    -- Prayer, formation, sacraments, and worship
    ('unified:52d100ca-22eb-55e1-b35c-a54a4773a754'),
    ('unified:d1d79bb2-d687-55b3-9d6f-b66fff854810'),
    ('unified:e573c2a9-b1b8-5d13-b71d-dde4e95b4870'),
    ('unified:c3720dc9-b5d7-51b7-983f-ee0c1d9d3dab'),
    ('unified:54c5c6ae-ae7a-560b-8f8c-eb327d0492c2'),
    ('unified:08c6be85-5c1f-5705-894d-5cec3a76d508'),
    ('unified:1a244f63-a71c-5ce7-964f-d7f7fe33f37e'),
    ('unified:6d8542f0-2b6e-54a8-bd30-4faf4a25d59b'),
    ('unified:2d581edf-7cb4-517b-9e99-682e426569d0');

INSERT OR IGNORE INTO concept(concept_id,preferred_label)
SELECT c.concept_id,c.preferred_label
FROM master.concept c JOIN restore_christianity_concept r USING(concept_id);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT p.concept_id,p.parent_concept_id,p.ordinal,p.display_order
FROM master.concept_parent p
JOIN restore_christianity_concept r ON r.concept_id=p.concept_id
JOIN concept parent ON parent.concept_id=p.parent_concept_id;

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT s.concept_id,s.system_id,s.selector,s.ordinal
FROM master.source_selector s
JOIN restore_christianity_concept r USING(concept_id)
WHERE s.system_id IN ('bisac','lcc');

INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT l.concept_id,l.label,l.ordinal
FROM master.source_label l JOIN restore_christianity_concept r USING(concept_id);

-- Short browse labels for restored schedule concepts.
UPDATE concept SET preferred_label='Early Christian Literature'
WHERE concept_id='unified:2b9f1e4f-6e2a-5a82-bb9d-a8b5cae7d709';
UPDATE concept SET preferred_label='Modern Christianity'
WHERE concept_id='unified:b09cca90-854f-5cb4-8092-ea267c378cf7';
UPDATE concept SET preferred_label='Toleration'
WHERE concept_id='unified:cb4a0a0e-5bcb-5f66-ac96-4edf163deb60';
UPDATE concept SET preferred_label='Doctrine & Dogma'
WHERE concept_id='unified:74db0a97-60c6-56bd-bc7c-27498015d201';
UPDATE concept SET preferred_label='Salvation'
WHERE concept_id='unified:da2b82e3-992d-5056-b5ed-99a183801264';

DROP TABLE restore_christianity_concept;
COMMIT;
DETACH DATABASE master;
