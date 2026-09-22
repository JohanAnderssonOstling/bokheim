-- Flatten redundant Slavic, Baltic, Albanian, and Russian literature
-- wrappers into the LCC-aligned Slavic & Eurasian group.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector (concept_id,selector)
SELECT 5589,selector FROM lcc_selector WHERE concept_id=4460;

CREATE TEMP TABLE slavic_promoted (concept_id INTEGER PRIMARY KEY);
INSERT INTO slavic_promoted (concept_id) VALUES
  (4463),(7609),(6284),
  (12490),(12491),(12492),(12493),(12494),(12495),(12496),(12497),
  (51867),(6417),(8104),(13657),
  (12522),(12519),(12520),(12518),(12525),(12524),(12521),(12523),
  (4466);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM slavic_promoted)
   OR parent_concept_id IN (4460,4464,4465,4467,4468,8151);

INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT concept_id,5589,0 FROM slavic_promoted;

DELETE FROM lcc_selector WHERE concept_id IN (4460,4464,4465,4467,4468,8151,7983);
DELETE FROM bisac_selector WHERE concept_id IN (4460,4464,4465,4467,4468,8151,7983);
DELETE FROM ddc_selector WHERE concept_id IN (4460,4464,4465,4467,4468,8151,7983);
DELETE FROM source_label WHERE concept_id IN (4460,4464,4465,4467,4468,8151,7983);
DELETE FROM concept_parent
WHERE concept_id IN (4460,4464,4465,4467,4468,8151,7983)
   OR parent_concept_id IN (4460,4464,4465,4467,4468,8151,7983);
DELETE FROM concept WHERE concept_id IN (4460,4464,4465,4467,4468,8151,7983);

DROP TABLE slavic_promoted;
COMMIT;
