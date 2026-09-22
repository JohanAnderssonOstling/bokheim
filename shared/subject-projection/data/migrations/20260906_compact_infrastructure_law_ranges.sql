-- Compact dense infrastructure/public-works blocks within each jurisdiction.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=5559 AND system_id='lcc' AND (
 selector GLOB 'KDC60*' OR selector GLOB 'KDK69*' OR selector GLOB 'KDK71*' OR
 selector GLOB 'KDK76*' OR selector GLOB 'KE52*' OR selector GLOB 'KFC4*' OR
 selector GLOB 'KFC5*' OR selector GLOB 'KFN54*' OR selector GLOB 'KFN55*' OR
 selector GLOB 'KIB1*' OR selector GLOB 'KIE10*' OR selector GLOB 'KIE170*' OR
 selector GLOB 'KJC61*' OR selector GLOB 'KJC69*' OR selector GLOB 'KJE891*' OR
 selector GLOB 'KVJ11*' OR selector GLOB 'KVJ120*'
);
INSERT INTO source_selector VALUES
 (5559,'lcc','KDC607..KDC610'),
 (5559,'lcc','KDK697..KDK713'),
 (5559,'lcc','KDK764..KDK769'),
 (5559,'lcc','KE5263..KE5284'),
 (5559,'lcc','KFC455.1..KFC497.2'),
 (5559,'lcc','KFC521..KFC531'),
 (5559,'lcc','KFN5457..KFN5534'),
 (5559,'lcc','KIB1092..KIB1160'),
 (5559,'lcc','KIB1702..KIB1704'),
 (5559,'lcc','KIE1076..KIE1098'),
 (5559,'lcc','KIE1700..KIE1704'),
 (5559,'lcc','KJC6138..KJC6155'),
 (5559,'lcc','KJC6951..KJC6970'),
 (5559,'lcc','KJE891.2..KJE891.22'),
 (5559,'lcc','KVJ1130..KVJ1200');

COMMIT;
