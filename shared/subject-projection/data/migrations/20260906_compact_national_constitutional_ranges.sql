-- Compact dense national constitutional-law blocks within each jurisdiction.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=13327 AND system_id='lcc' AND (
 selector GLOB 'KD39*' OR selector GLOB 'KD40*' OR selector GLOB 'KE42*' OR
 selector GLOB 'KIA23*' OR selector GLOB 'KIA24*' OR selector GLOB 'KIA38*' OR
 selector GLOB 'KIB18*' OR selector GLOB 'KIB19*' OR
 selector GLOB 'KIE18*' OR selector GLOB 'KIE19*' OR selector GLOB 'KIE20*' OR selector GLOB 'KIE21*' OR
 selector GLOB 'KJA29*' OR selector GLOB 'KJA30*' OR
 selector GLOB 'KJC44*' OR selector GLOB 'KJC50*' OR selector GLOB 'KJC51*' OR selector GLOB 'KJC52*' OR selector GLOB 'KJC53*' OR selector GLOB 'KJC54*' OR
 selector GLOB 'KK455*' OR selector GLOB 'KK47*' OR selector GLOB 'KK504*' OR selector GLOB 'KK541*' OR
 selector GLOB 'KLA21*' OR selector GLOB 'KLA22*' OR
 selector GLOB 'KNX21*' OR selector GLOB 'KNX22*' OR selector GLOB 'KNX23*' OR
 selector GLOB 'KVJ17*' OR selector GLOB 'KVJ18*'
);
INSERT INTO source_selector VALUES
 (13327,'lcc','KD3995..KD4018'),
 (13327,'lcc','KE4238..KE4254'),
 (13327,'lcc','KIA383..KIA392'),
 (13327,'lcc','KIA2392..KIA2434'),
 (13327,'lcc','KIB1810..KIB1840'),
 (13327,'lcc','KIB1918..KIB1952'),
 (13327,'lcc','KIE1877..KIE1950'),
 (13327,'lcc','KIE2095..KIE2126'),
 (13327,'lcc','KJA2910..KJA3050'),
 (13327,'lcc','KJC4420..KJC4447'),
 (13327,'lcc','KJC5034..KJC5075'),
 (13327,'lcc','KJC5095..KJC5222'),
 (13327,'lcc','KJC5245..KJC5456'),
 (13327,'lcc','KK4553..KK4558'),
 (13327,'lcc','KK4734..KK4781'),
 (13327,'lcc','KK5041..KK5044'),
 (13327,'lcc','KK5410..KK5416'),
 (13327,'lcc','KLA2112..KLA2150'),
 (13327,'lcc','KLA2240..KLA2260'),
 (13327,'lcc','KNX2130..KNX2140'),
 (13327,'lcc','KNX2270..KNX2305'),
 (13327,'lcc','KVJ1717..KVJ1735'),
 (13327,'lcc','KVJ1772..KVJ1817');

COMMIT;
