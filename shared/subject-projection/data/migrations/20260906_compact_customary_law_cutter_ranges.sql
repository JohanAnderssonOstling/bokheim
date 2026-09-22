-- Compact repeated title/author Cutter enumerations under stable customary and
-- indigenous-law class numbers. Numeric subtopics remain exact.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id=5879 AND system_id='lcc' AND instr(selector,'..')=0 AND (
 selector GLOB 'KI513.[A-Z]*' OR selector GLOB 'KI565.[A-Z]*' OR
 selector GLOB 'KIA4.2.[A-Z]*' OR selector GLOB 'KIA4.5.[A-Z]*' OR
 selector GLOB 'KIA20.[A-Z]*' OR selector GLOB 'KIM6.2.[A-Z]*' OR
 selector GLOB 'KIM75.[A-Z]*' OR selector GLOB 'KIQ6045.[A-Z]*' OR
 selector GLOB 'KIQ6047.[A-Z]*' OR selector GLOB 'KIQ6076.[A-Z]*' OR
 selector GLOB 'KIQ6118.[A-Z]*' OR selector GLOB 'KIQ6164.[A-Z]*' OR
 selector GLOB 'KIQ6172.[A-Z]*' OR selector GLOB 'KIQ6255.[A-Z]*' OR
 selector GLOB 'KIQ9983.[A-Z]*'
);
INSERT INTO source_selector VALUES
 (5879,'lcc','KI513.A..KI513.Z'),
 (5879,'lcc','KI565.A..KI565.Z'),
 (5879,'lcc','KIA4.2.A..KIA4.2.Z'),
 (5879,'lcc','KIA4.5.A..KIA4.5.Z'),
 (5879,'lcc','KIA20.A..KIA20.Z'),
 (5879,'lcc','KIM6.2.A..KIM6.2.Z'),
 (5879,'lcc','KIM75.A..KIM75.Z'),
 (5879,'lcc','KIQ6045.A..KIQ6045.Z'),
 (5879,'lcc','KIQ6047.A..KIQ6047.Z'),
 (5879,'lcc','KIQ6076.A..KIQ6076.Z'),
 (5879,'lcc','KIQ6118.A..KIQ6118.Z'),
 (5879,'lcc','KIQ6164.A..KIQ6164.Z'),
 (5879,'lcc','KIQ6172.A..KIQ6172.Z'),
 (5879,'lcc','KIQ6255.A..KIQ6255.Z'),
 (5879,'lcc','KIQ9983.A..KIQ9983.Z');

COMMIT;
