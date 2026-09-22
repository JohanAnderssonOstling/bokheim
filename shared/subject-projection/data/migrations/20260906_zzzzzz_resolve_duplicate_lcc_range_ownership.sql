-- An LCC range has one curated owner. Prefer an existing descendant over an
-- ancestor carrying the identical range, then resolve the remaining peer
-- duplicates in favor of the subject most closely matching the LCC caption.
BEGIN;

WITH RECURSIVE ancestor(child, ancestor) AS (
    SELECT concept_id, parent_concept_id FROM concept_parent
    UNION
    SELECT ancestor.child, edge.parent_concept_id
    FROM ancestor
    JOIN concept_parent edge ON edge.concept_id = ancestor.ancestor
)
DELETE FROM source_selector AS loser
WHERE loser.system_id='lcc'
  AND instr(loser.selector,'..')>0
  AND EXISTS (
      SELECT 1
      FROM source_selector AS winner
      JOIN ancestor
        ON ancestor.child=winner.concept_id
       AND ancestor.ancestor=loser.concept_id
      WHERE winner.system_id=loser.system_id
        AND winner.selector=loser.selector
  );

DELETE FROM source_selector
WHERE system_id='lcc' AND (concept_id,selector) IN (
  VALUES
    (5463,'HV8031..HV8080'),
    (5463,'HV8081..HV8099'),
    (13821,'JC311..JC314'),
    (5281,'K1241..K1287'),
    (5877,'K288..K296'),
    (5281,'K3310..K3329'),
    (5281,'K3941..K3974'),
    (5281,'K3978..K3992'),
    (5281,'K5011..K5350'),
    (5281,'K5575..K5582'),
    (5878,'KJA2970..KJA3037'),
    (5878,'KJA3040..KJA3050'),
    (5317,'KJC1010..KJC1098'),
    (5317,'KJC1251..KJC1437'),
    (5317,'KJC1491..KJC1905'),
    (5317,'KJC2188..KJC2255'),
    (5317,'KJC2256..KJC2289'),
    (5317,'KJC2435..KJC2446'),
    (5317,'KJC2655..KJC2700'),
    (5317,'KJC2706..KJC2712'),
    (5317,'KJC2799..KJC2830'),
    (5549,'KJC40..KJC40.22'),
    (5317,'KJC4175..KJC4353'),
    (5317,'KJC4431..KJC4435.8'),
    (5317,'KJC5105..KJC5112'),
    (5317,'KJC5259..KJC5445'),
    (5317,'KJC6428..KJC6435'),
    (5317,'KJC6456..KJC6497'),
    (5317,'KJC6554..KJC6559'),
    (5317,'KJC6569..KJC6830'),
    (5317,'KJC6590..KJC6681'),
    (5317,'KJC6695..KJC6697'),
    (5317,'KJC6700..KJC6720'),
    (5317,'KJC6848..KJC6858'),
    (5317,'KJC7431..KJC7435'),
    (5317,'KJC7475..KJC7495'),
    (5318,'KJE6132..KJE6155'),
    (5318,'KJE6266..KJE6321'),
    (5318,'KJE6428..KJE6537'),
    (5318,'KJE6569..KJE6830'),
    (5318,'KJE857..KJE859'),
    (5318,'KJE864..KJE865'),
    (5318,'KJE867..KJE869'),
    (5300,'KZ118..KZ194'),
    (5300,'KZ1298..KZ1304'),
    (5300,'KZ5647..KZ5686'),
    (5300,'KZ5870..KZ5893'),
    (5300,'KZ7145..KZ7177'),
    (5300,'KZ7180..KZ7188'),
    (4376,'PN4699..PN5650'),
    (13677,'PN851..PN884'),
    (6513,'PR111..PR120'),
    (7958,'PR500..PR614'),
    (8324,'PR621..PR744'),
    (6356,'PR951..PR981.2'),
    (13323,'Q174..Q175.3'),
    (13329,'QA267..QA268.5'),
    (5564,'TA1..TA2040'),
    (13770,'VM1..VM989'),
    (4750,'Z716..Z716.15')
);

COMMIT;
