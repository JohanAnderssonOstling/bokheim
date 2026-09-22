BEGIN;

WITH wanted(label) AS (VALUES
 ('Turin History'),('Friuli–Venezia Giulia History'),('Palermo History'),
 ('Calabria History'),('Messina History'),('Parma History'),('Ravenna History'),
 ('Perugia History'),('Liguria History'),('Capri History'),('Lucca History'),
 ('Aosta Valley History'),('Lazio History'),('Emilia-Romagna History'),
 ('Padua History'),('Bari History'),('Basilicata History'),('Arezzo History'),
 ('Tarragona History'),('Seville History'),('Valladolid History'),
 ('Girona History'),('Lleida History'),('Madrid History'),
 ('Santiago de Compostela History'),('Zaragoza History'),
 ('Cantabrian Mountains History'),('Álava History'),('Huelva History'),
 ('Castilla-La Mancha History'),('La Rioja History'),('Pyrenees History'),
 ('Bilbao History'),('Salamanca History'),('Almería History'),
 ('Alicante History'),('Valencia History'),('Gibraltar History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13851,0 FROM concept WHERE preferred_label IN (
 'Turin History','Friuli–Venezia Giulia History','Palermo History',
 'Calabria History','Messina History','Parma History','Ravenna History',
 'Perugia History','Liguria History','Capri History','Lucca History',
 'Aosta Valley History','Lazio History','Emilia-Romagna History','Padua History',
 'Bari History','Basilicata History','Arezzo History');

INSERT OR IGNORE INTO lcc_range SELECT 'DG','975.T93','975.T97',concept_id FROM concept WHERE preferred_label='Turin History';
INSERT OR IGNORE INTO lcc_range SELECT 'DG','975.F85','975.F855',concept_id FROM concept WHERE preferred_label='Friuli–Venezia Giulia History';
INSERT OR IGNORE INTO lcc_range SELECT 'DG','975.P19','975.P21',concept_id FROM concept WHERE preferred_label='Palermo History';
INSERT OR IGNORE INTO lcc_range SELECT 'DG','975.C13','975.C16',concept_id FROM concept WHERE preferred_label='Calabria History';
INSERT OR IGNORE INTO lcc_range SELECT 'DG','975.M53','975.M534',concept_id FROM concept WHERE preferred_label='Messina History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.P25' FROM concept WHERE preferred_label='Parma History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.R25' FROM concept WHERE preferred_label='Ravenna History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.P4' FROM concept WHERE preferred_label='Perugia History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.L68' FROM concept WHERE preferred_label='Liguria History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.C2' FROM concept WHERE preferred_label='Capri History';
INSERT OR IGNORE INTO lcc_range SELECT 'DG','975.L8','975.L82',concept_id FROM concept WHERE preferred_label='Lucca History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.A6' FROM concept WHERE preferred_label='Aosta Valley History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.L245' FROM concept WHERE preferred_label='Lazio History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.E53' FROM concept WHERE preferred_label='Emilia-Romagna History';
INSERT OR IGNORE INTO lcc_range SELECT 'DG','975.P12','975.P13',concept_id FROM concept WHERE preferred_label='Padua History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.B25' FROM concept WHERE preferred_label='Bari History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.B3' FROM concept WHERE preferred_label='Basilicata History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DG975.A7' FROM concept WHERE preferred_label='Arezzo History';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13847,0 FROM concept WHERE preferred_label IN (
 'Tarragona History','Seville History','Valladolid History','Girona History',
 'Lleida History','Madrid History','Santiago de Compostela History',
 'Zaragoza History','Cantabrian Mountains History','Álava History','Huelva History',
 'Castilla-La Mancha History','La Rioja History','Pyrenees History','Bilbao History',
 'Salamanca History','Almería History','Alicante History','Valencia History',
 'Gibraltar History');

INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.T11','302.T15',concept_id FROM concept WHERE preferred_label='Tarragona History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.T3' FROM concept WHERE preferred_label='Tarragona History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.S56','302.S6',concept_id FROM concept WHERE preferred_label='Seville History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','402.S36','402.S48',concept_id FROM concept WHERE preferred_label='Seville History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.V31' FROM concept WHERE preferred_label='Valladolid History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.V3' FROM concept WHERE preferred_label='Valladolid History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.G21','302.G3',concept_id FROM concept WHERE preferred_label='Girona History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.G4' FROM concept WHERE preferred_label='Girona History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.L46' FROM concept WHERE preferred_label='Lleida History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.L33' FROM concept WHERE preferred_label='Lleida History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.M1' FROM concept WHERE preferred_label='Madrid History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','350','374',concept_id FROM concept WHERE preferred_label='Madrid History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.S23' FROM concept WHERE preferred_label='Santiago de Compostela History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.S41','302.S465',concept_id FROM concept WHERE preferred_label='Zaragoza History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','402.S3','402.S32',concept_id FROM concept WHERE preferred_label='Zaragoza History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.C54' FROM concept WHERE preferred_label='Cantabrian Mountains History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.A02' FROM concept WHERE preferred_label='Álava History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.H7' FROM concept WHERE preferred_label='Huelva History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.C553' FROM concept WHERE preferred_label='Castilla-La Mancha History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.L18' FROM concept WHERE preferred_label='La Rioja History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.P8' FROM concept WHERE preferred_label='Pyrenees History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.B5' FROM concept WHERE preferred_label='Bilbao History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.S11','302.S25',concept_id FROM concept WHERE preferred_label='Salamanca History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.S1' FROM concept WHERE preferred_label='Salamanca History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP302.A33' FROM concept WHERE preferred_label='Almería History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.A11','302.A25',concept_id FROM concept WHERE preferred_label='Alicante History';
INSERT OR IGNORE INTO lcc_selector SELECT concept_id,'DP402.A42' FROM concept WHERE preferred_label='Alicante History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.V11','302.V25',concept_id FROM concept WHERE preferred_label='Valencia History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','402.V15','402.V25',concept_id FROM concept WHERE preferred_label='Valencia History';
INSERT OR IGNORE INTO lcc_range SELECT 'DP','302.G31','302.G41',concept_id FROM concept WHERE preferred_label='Gibraltar History';

COMMIT;
