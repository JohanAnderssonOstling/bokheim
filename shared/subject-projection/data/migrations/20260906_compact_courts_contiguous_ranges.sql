-- Compact conservative, visibly contiguous court-organization blocks.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=13308 AND system_id='lcc' AND (
 selector GLOB 'KBP158*' OR selector GLOB 'KBS395*' OR selector GLOB 'KBS572*' OR
 selector IN ('KD330','KD331','KE203','KE204','KIA424','KIA426') OR
 selector GLOB 'KDE5*' OR selector GLOB 'KJC40*' OR
 selector GLOB 'KK94*' OR selector GLOB 'KKA94*'
);
INSERT INTO source_selector VALUES
 (13308,'lcc','KBP1584..KBP1595'),
 (13308,'lcc','KBS395.3..KBS395.7'),
 (13308,'lcc','KBS572..KBS572.5'),
 (13308,'lcc','KD330..KD331'),
 (13308,'lcc','KDE515..KDE530'),
 (13308,'lcc','KE203..KE204'),
 (13308,'lcc','KIA424..KIA426'),
 (13308,'lcc','KJC40..KJC40.22'),
 (13308,'lcc','KK9445..KK9480'),
 (13308,'lcc','KKA9445..KKA9480');

COMMIT;
