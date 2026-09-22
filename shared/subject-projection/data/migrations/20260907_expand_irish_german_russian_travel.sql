BEGIN;
CREATE TEMP TABLE _travel(label TEXT,parent_id INTEGER,selector TEXT);
INSERT INTO _travel VALUES
 ('Irish Place Names',2463,'DA979'),('Irish Travel Directories',2463,'DA979.5'),
 ('Irish Guidebooks',2463,'DA980'),('Pictorial Ireland',2463,'DA982'),
 ('Irish Castles & Historic Houses',2463,'DA985'),('Dungory Castle',2463,'DA987.D8'),
 ('Islands of Ireland Travel',2463,'DA988'),
 ('West German Travel',2460,'DD258.25'),('East German Travel',2460,'DD280.5'),
 ('East German Travel',2460,'DD280.8'),('Württemberg Travel',2460,'DD801.W627'),
 ('Württemberg Travel',2460,'DD801.W634'),('Berlin Travel',2460,'DD859'),
 ('Munich Travel',2460,'DD901.M76'),
 ('Russian Federation Travel',2493,'DK510.22'),('Russian Federation Travel',2493,'DK510.29'),
 ('Saint Petersburg Travel',2493,'DK549'),('Moscow Travel',2493,'DK597'),
 ('Siberian Travel',2493,'DK754');
DELETE FROM lcc_selector WHERE concept_id IN (2463,2460,2493) AND selector IN (SELECT selector FROM _travel);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code FROM lcc_range WHERE concept_id IN (2463,2460,2493);
DELETE FROM lcc_range WHERE concept_id IN (2463,2460,2493);
INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _travel s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label,parent_id FROM _travel) s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _travel s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _travel;
COMMIT;
