-- Compact small repeated Cutter groups where the base number itself identifies
-- the curated leaf. Deliberately exclude topical Cutter alphabets such as HC79,
-- QA76.9, and literary author schedules.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND instr(selector,'..')=0 AND (
 (concept_id=12668 AND selector GLOB 'RC552.[A-Z]*') OR
 (concept_id=5876 AND selector GLOB 'KJC435.[A-Z]*') OR
 (concept_id=5310 AND selector GLOB 'KK1018.2.[A-Z]*') OR
 (concept_id=5557 AND selector GLOB 'KIA2287.[A-Z]*') OR
 (concept_id=5557 AND selector GLOB 'KIA337.3.[A-Z]*') OR
 (concept_id=5615 AND selector GLOB 'KEQ694.[A-Z]*')
);
INSERT INTO source_selector VALUES
 (12668,'lcc','RC552.A..RC552.Z'),
 (5876,'lcc','KJC435.A..KJC435.Z'),
 (5310,'lcc','KK1018.2.A..KK1018.2.Z'),
 (5557,'lcc','KIA2287.A..KIA2287.Z'),
 (5557,'lcc','KIA337.3.A..KIA337.3.Z'),
 (5615,'lcc','KEQ694.A..KEQ694.Z');

COMMIT;
