BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Aargau History'), ('Appenzell History'), ('Jura History'),
  ('Basel-Landschaft History'), ('Basel-Stadt History'), ('Bern History'),
  ('Fribourg History'), ('Geneva History'), ('Glarus History'),
  ('Graubünden History'), ('Lucerne History'), ('Neuchâtel History'),
  ('St. Gallen History'), ('Schaffhausen History'), ('Schwyz History'),
  ('Solothurn History'), ('Thurgau History'), ('Ticino History'),
  ('Unterwalden History'), ('Uri History'), ('Valais History'),
  ('Vaud History'), ('Zug History'), ('Zürich History'),
  ('Swiss Alps History'), ('Swiss Social & Cultural History'),
  ('Swiss Ethnography'), ('Swiss Political & Diplomatic History');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 3602, 0 FROM concept WHERE preferred_label IN (
  'Aargau History', 'Appenzell History', 'Jura History',
  'Basel-Landschaft History', 'Basel-Stadt History', 'Bern History',
  'Fribourg History', 'Geneva History', 'Glarus History',
  'Graubünden History', 'Lucerne History', 'Neuchâtel History',
  'St. Gallen History', 'Schaffhausen History', 'Schwyz History',
  'Solothurn History', 'Thurgau History', 'Ticino History',
  'Unterwalden History', 'Uri History', 'Valais History',
  'Vaud History', 'Zug History', 'Zürich History',
  'Swiss Alps History', 'Swiss Social & Cultural History',
  'Swiss Ethnography', 'Swiss Political & Diplomatic History'
);

CREATE TEMP TABLE swiss_canton_move(start_value TEXT, end_value TEXT, label TEXT);
INSERT INTO swiss_canton_move VALUES
  ('301','320.35','Aargau History'), ('321','340.35','Appenzell History'),
  ('341','360.35','Jura History'), ('361','380.35','Basel-Landschaft History'),
  ('381','400.35','Basel-Stadt History'), ('401','420.35','Bern History'),
  ('421','440.35','Fribourg History'), ('441','460.35','Geneva History'),
  ('461','480.35','Glarus History'), ('481','500.35','Graubünden History'),
  ('501','520.35','Lucerne History'), ('521','540.35','Neuchâtel History'),
  ('541','560.35','St. Gallen History'), ('561','580.35','Schaffhausen History'),
  ('581','600.35','Schwyz History'), ('601','620.35','Solothurn History'),
  ('621','640.35','Thurgau History'), ('641','660.35','Ticino History'),
  ('661','680.35','Unterwalden History'), ('681','700.35','Uri History'),
  ('701','720.35','Valais History'), ('721','740.35','Vaud History'),
  ('741','760.35','Zug History'), ('781','800.35','Zürich History');

UPDATE lcc_range
SET concept_id = (SELECT c.concept_id FROM swiss_canton_move m
                  JOIN concept c ON c.preferred_label = m.label
                  WHERE m.start_value = lcc_range.start_value
                    AND m.end_value = lcc_range.end_value)
WHERE class_letters = 'DQ'
  AND EXISTS (SELECT 1 FROM swiss_canton_move m
              WHERE m.start_value = lcc_range.start_value
                AND m.end_value = lcc_range.end_value);

UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept WHERE preferred_label='Swiss Alps History')
WHERE class_letters='DQ' AND start_value='820' AND end_value='829';
UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept WHERE preferred_label='Swiss Political & Diplomatic History')
WHERE class_letters='DQ' AND start_value='68' AND end_value='76';
UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept WHERE preferred_label='Swiss Ethnography')
WHERE class_letters='DQ' AND start_value='48' AND end_value='49.5';

INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DQ36' FROM concept WHERE preferred_label='Swiss Social & Cultural History';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2472,'DQ16');

DROP TABLE swiss_canton_move;
COMMIT;
