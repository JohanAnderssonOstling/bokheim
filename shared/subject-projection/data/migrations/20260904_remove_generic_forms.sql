-- Remove generic publication-form branches throughout the curated taxonomy.
-- Biography is intentionally retained. Broader named subjects such as
-- Literary Collections and Reference & Information are unaffected.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE generic_form_label(label TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO generic_form_label VALUES
 ('general works'),
 ('general special'),
 ('congresses'),
 ('periodicals. societies. serials'),
 ('periodicals. serials'),
 ('addresses, essays, lectures'),
 ('juvenile works'),
 ('directories'),
 ('collected works (nonserial)'),
 ('dictionaries and encyclopedias'),
 ('dictionaries. encyclopedias'),
 ('study and teaching'),
 ('study and teaching. research'),
 ('collections'),
 ('popular works'),
 ('reference'),
 ('general'),
 ('general works, treatises, and textbooks');

CREATE TEMP TABLE generic_form_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT concept.concept_id
 FROM concept
 JOIN generic_form_label form ON form.label=lower(trim(concept.preferred_label))
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO generic_form_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM generic_form_detail)
   OR parent_concept_id IN (SELECT concept_id FROM generic_form_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM generic_form_detail);

DROP TABLE generic_form_detail;
DROP TABLE generic_form_label;
COMMIT;
