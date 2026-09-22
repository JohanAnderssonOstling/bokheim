-- Store every LCC range with one shared class field. All curated ranges have
-- the same class at both ends; endpoint remainders stay as text so decimal
-- numbers, Cutters, and suffixes remain lossless.
BEGIN;

CREATE TABLE lcc_range(
    class_letters TEXT NOT NULL
        CHECK(length(class_letters) BETWEEN 1 AND 3)
        CHECK(class_letters=upper(class_letters))
        CHECK(class_letters NOT GLOB '*[^A-Z]*'),
    start_value TEXT NOT NULL CHECK(length(trim(start_value))>0),
    end_value TEXT NOT NULL CHECK(length(trim(end_value))>0),
    concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
    PRIMARY KEY(class_letters,start_value,end_value)
) STRICT, WITHOUT ROWID;

CREATE INDEX lcc_range_concept_idx ON lcc_range(concept_id);

WITH encoded AS (
    SELECT concept_id,
           replace(trim(substr(selector,1,instr(selector,'..')-1)),' ','') start_code,
           replace(trim(substr(selector,instr(selector,'..')+2)),' ','') end_code
    FROM source_selector
    WHERE system_id='lcc' AND instr(selector,'..')>0
), parsed AS (
    SELECT *, CASE
      WHEN substr(start_code,2,1) GLOB '[0-9]' THEN 1
      WHEN substr(start_code,3,1) GLOB '[0-9]' THEN 2
      ELSE 3 END class_length
    FROM encoded
)
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT substr(start_code,1,class_length),
       substr(start_code,class_length+1),
       substr(end_code,class_length+1),
       concept_id
FROM parsed
WHERE substr(end_code,1,class_length)=substr(start_code,1,class_length);

CREATE TEMP TABLE lcc_range_conversion_guard(ok INTEGER NOT NULL CHECK(ok=1));
INSERT INTO lcc_range_conversion_guard
SELECT (SELECT count(*) FROM lcc_range) =
       (SELECT count(*) FROM source_selector
        WHERE system_id='lcc' AND instr(selector,'..')>0);
DROP TABLE lcc_range_conversion_guard;

DELETE FROM source_selector
WHERE system_id='lcc' AND instr(selector,'..')>0;

UPDATE taxonomy_meta SET value='6' WHERE key='format_version';

COMMIT;
