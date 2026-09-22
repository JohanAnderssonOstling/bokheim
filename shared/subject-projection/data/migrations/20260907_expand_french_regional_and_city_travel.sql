BEGIN;
CREATE TEMP TABLE _travel(label TEXT,selector TEXT);
INSERT INTO _travel VALUES
 ('Northern France Travel','DC601.3'),('Eastern France Travel','DC603.3'),
 ('Central France Travel','DC605.3'),('Southern France Travel','DC607.3'),
 ('French Riviera Travel','DC608.3'),('Western France Travel','DC609.3'),
 ('Aquitaine Travel','DC611.A65'),('Auvergne Travel','DC611.A942'),
 ('French Basque Country Travel','DC611.B313'),('Béarn Travel','DC611.B372'),
 ('Burgundy Travel','DC611.B773'),('Brittany Travel','DC611.B848'),
 ('Champagne Travel','DC611.C451'),('Corsica Travel','DC611.C811'),('Corsica Travel','DC611.C812'),
 ('Dauphiné Travel','DC611.D248'),('Franche-Comté Travel','DC611.F813'),
 ('Gascony & Guyenne Travel','DC611.G26'),('Languedoc Travel','DC611.L292'),
 ('Lyonnais Travel','DC611.L96'),('Normandy Travel','DC611.N848'),
 ('Poitou Travel','DC611.P74'),('Provence Travel','DC611.P958'),
 ('Touraine Travel','DC611.T728'),
 ('Alsace-Lorraine Travel','DC634'),('Alsace Travel','DC648.3'),('Lorraine Travel','DC653.3'),
 ('Paris Travel','DC707'),('Paris Travel','DC708'),
 ('Arles Travel','DC801.A71'),('Dijon Travel','DC801.D6'),
 ('Lyon Travel','DC801.L963'),('Orléans Travel','DC801.O63');

DELETE FROM lcc_selector WHERE concept_id=2459
 AND selector IN (SELECT selector FROM _travel);
-- Old imports also emitted these punctuation-free pseudo-codes.
DELETE FROM lcc_selector WHERE concept_id=2459
 AND selector IN ('DC611B773','DC611C812','DC611L292','DC611P958','DC801.D60');

-- French Travel is now a grouping; its former permissive range is retained
-- only as an exact printed notation.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2459,'DC21-29.3');
DELETE FROM lcc_range WHERE concept_id=2459;

INSERT INTO concept(preferred_label)
SELECT DISTINCT label FROM _travel s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,2459,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label FROM _travel) s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=2459);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _travel s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _travel;
COMMIT;
