-- Hundreds of title/author cutters enumerate works under the same official
-- jurisdiction-specific administrative-law numbers. Replace them with one
-- Cutter range per number.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id=11746 AND system_id='lcc' AND instr(selector,'..')=0;
INSERT INTO source_selector VALUES
 (11746,'lcc','KJG2935.A..KJG2935.Z'),
 (11746,'lcc','KJH293.5.A..KJH293.5.Z'),
 (11746,'lcc','KJK2935.A..KJK2935.Z'),
 (11746,'lcc','KJM2935.A..KJM2935.Z'),
 (11746,'lcc','KJR2935.A..KJR2935.Z'),
 (11746,'lcc','KJS2935.A..KJS2935.Z'),
 (11746,'lcc','KJT2935.A..KJT2935.Z'),
 (11746,'lcc','KKE2935.A..KKE2935.Z'),
 (11746,'lcc','KKF2935.A..KKF2935.Z'),
 (11746,'lcc','KKI2935.A..KKI2935.Z'),
 (11746,'lcc','KKJ293.5.A..KKJ293.5.Z'),
 (11746,'lcc','KKJ7935.A..KKJ7935.Z'),
 (11746,'lcc','KKK293.5.A..KKK293.5.Z'),
 (11746,'lcc','KKL293.5.A..KKL293.5.Z'),
 (11746,'lcc','KKN2935.A..KKN2935.Z'),
 (11746,'lcc','KKP2935.A..KKP2935.Z'),
 (11746,'lcc','KKQ2935.A..KKQ2935.Z'),
 (11746,'lcc','KKR2935.A..KKR2935.Z'),
 (11746,'lcc','KKS293.5.A..KKS293.5.Z'),
 (11746,'lcc','KKV2935.A..KKV2935.Z'),
 (11746,'lcc','KKX2935.A..KKX2935.Z'),
 (11746,'lcc','KKY2935.A..KKY2935.Z');

COMMIT;
