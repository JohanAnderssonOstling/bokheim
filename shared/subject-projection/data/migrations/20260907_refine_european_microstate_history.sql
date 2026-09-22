BEGIN;

-- Andorra has three explicit chronological selectors and a locality block.
UPDATE concept SET preferred_label='Andorran History' WHERE concept_id=3554;
WITH wanted(label) AS (VALUES
 ('Early Andorra'),('Medieval & Early Modern Andorra'),('Modern Andorra'),
 ('Andorran Local History'),('Andorran Travel'),
 ('Liechtenstein History'),('Liechtenstein Historical Reference'),
 ('Liechtenstein Antiquities'),('Liechtenstein General History'),
 ('Liechtenstein Foreign Relations History'),('Liechtenstein Local History'),
 ('Liechtenstein Travel'),('Monaco History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label=label);

DELETE FROM lcc_range WHERE concept_id=3554 AND class_letters='DC';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3554,0 FROM concept WHERE preferred_label IN
 ('Early Andorra','Medieval & Early Modern Andorra','Modern Andorra','Andorran Local History');
INSERT INTO lcc_selector SELECT concept_id,'DC926' FROM concept WHERE preferred_label='Early Andorra';
INSERT INTO lcc_selector SELECT concept_id,'DC927' FROM concept WHERE preferred_label='Medieval & Early Modern Andorra';
INSERT INTO lcc_selector SELECT concept_id,'DC928' FROM concept WHERE preferred_label='Modern Andorra';
INSERT INTO lcc_range SELECT 'DC','932.A','932.Z',concept_id FROM concept WHERE preferred_label='Andorran Local History';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Andorran Travel';
INSERT INTO lcc_selector SELECT concept_id,'DC924' FROM concept WHERE preferred_label='Andorran Travel';

-- Liechtenstein is a standalone official DB881-DB898 history schedule, not
-- generic Central European or Austrian history.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3552,0 FROM concept WHERE preferred_label='Liechtenstein History';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,r.concept_id,0 FROM concept c JOIN concept r
WHERE r.preferred_label='Liechtenstein History' AND c.preferred_label IN (
 'Liechtenstein Historical Reference','Liechtenstein Antiquities',
 'Liechtenstein General History','Liechtenstein Foreign Relations History',
 'Liechtenstein Local History');
INSERT INTO lcc_range SELECT 'DB','881','887',concept_id FROM concept WHERE preferred_label='Liechtenstein Historical Reference';
INSERT INTO lcc_selector SELECT concept_id,'DB889' FROM concept WHERE preferred_label='Liechtenstein Antiquities';
INSERT INTO lcc_range SELECT 'DB','891','892.999',concept_id FROM concept WHERE preferred_label='Liechtenstein General History';
INSERT INTO lcc_range SELECT 'DB','893','894.Z',concept_id FROM concept WHERE preferred_label='Liechtenstein Foreign Relations History';
INSERT INTO lcc_range SELECT 'DB','898.A','898.Z',concept_id FROM concept WHERE preferred_label='Liechtenstein Local History';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Liechtenstein Travel';
INSERT INTO lcc_selector SELECT concept_id,'DB885' FROM concept WHERE preferred_label='Liechtenstein Travel';
INSERT INTO lcc_selector SELECT concept_id,'DB888' FROM concept WHERE preferred_label='Liechtenstein Travel';

-- Monaco has a compact undivided schedule and no observed matched books, so a
-- country leaf is preferable to artificial empty subdivisions.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5541,0 FROM concept WHERE preferred_label='Monaco History';
INSERT INTO lcc_range SELECT 'DC','941','947',concept_id FROM concept WHERE preferred_label='Monaco History';

COMMIT;
