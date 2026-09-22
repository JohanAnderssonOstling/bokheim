PRAGMA foreign_keys=ON;
BEGIN;

-- Reuse the existing Periodicals concept and expose it in Reference as well.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (12547,4671,1);

CREATE TEMP TABLE _ddc_periodical_language(label TEXT PRIMARY KEY,selector TEXT UNIQUE);
INSERT INTO _ddc_periodical_language VALUES
 ('American English','051'),
 ('English','052'),
 ('Other Germanic','053'),
 ('French, Occitan & Catalan','054'),
 ('Italian & Romanian','055'),
 ('Spanish & Portuguese','056'),
 ('Slavic','057'),
 ('Scandinavian','058'),
 ('Other Languages','059');

INSERT INTO concept(preferred_label)
SELECT g.label FROM _ddc_periodical_language g
WHERE NOT EXISTS(
 SELECT 1 FROM concept_parent cp JOIN concept c ON c.concept_id=cp.concept_id
 WHERE cp.parent_concept_id=12547 AND c.preferred_label=g.label
);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,12547,0
FROM _ddc_periodical_language g JOIN concept c ON c.preferred_label=g.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent cp WHERE cp.concept_id=c.concept_id)
  AND c.concept_id=(SELECT max(c2.concept_id) FROM concept c2 WHERE c2.preferred_label=g.label);

INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
VALUES ('050',12547);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT g.selector,c.concept_id
FROM _ddc_periodical_language g
JOIN concept_parent cp ON cp.parent_concept_id=12547
JOIN concept c ON c.concept_id=cp.concept_id AND c.preferred_label=g.label;

DROP TABLE _ddc_periodical_language;
COMMIT;
