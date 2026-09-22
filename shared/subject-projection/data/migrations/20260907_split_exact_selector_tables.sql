BEGIN;

CREATE TABLE lcc_selector(
  concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
  selector TEXT NOT NULL CHECK(length(trim(selector))>0),
  PRIMARY KEY(concept_id,selector)
) STRICT, WITHOUT ROWID;
CREATE INDEX lcc_selector_lookup_idx ON lcc_selector(selector,concept_id);

CREATE TABLE bisac_selector(
  concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
  selector TEXT NOT NULL CHECK(length(trim(selector))>0),
  PRIMARY KEY(concept_id,selector)
) STRICT, WITHOUT ROWID;
CREATE INDEX bisac_selector_lookup_idx ON bisac_selector(selector,concept_id);

INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,selector FROM source_selector WHERE system_id='lcc';
INSERT INTO bisac_selector(concept_id,selector)
SELECT concept_id,selector FROM source_selector WHERE system_id='bisac';

DROP TABLE source_selector;
UPDATE taxonomy_meta SET value='7' WHERE key='format_version';

COMMIT;
