PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- LCC Schedule H: Terrorism, including general works, geographic treatment,
-- individual incidents, and biological/chemical/maritime/narco/nuclear forms.
-- This curated node is a leaf, so the complete subtree is intentionally broad.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:b5368b92-7460-5ef7-a7a0-29fd1ab158a0','lcc',
       'HV6430..HV6433.88@5',0
WHERE NOT EXISTS (
 SELECT 1 FROM source_selector
 WHERE concept_id='unified:b5368b92-7460-5ef7-a7a0-29fd1ab158a0'
   AND system_id='lcc' AND selector='HV6430..HV6433.88@5'
);

COMMIT;
