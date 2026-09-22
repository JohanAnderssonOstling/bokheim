BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Welsh Biographies'),('Welsh Antiquities'),('Welsh Social & Cultural History'),
 ('Welsh Ethnography'),('Glamorgan History'),('Pembrokeshire History'),
 ('Snowdonia History'),('Cardiff History'),
 ('Albanian Antiquities'),('Albanian Social & Cultural History'),
 ('Albanian Ethnography'),('Albanians Abroad'),('Albanian Biographies'),
 ('Albanian Historiography'),('Albanian Political History'),
 ('Albanian Foreign Relations History'),('Post-Communist Albanian Political History'),
 ('Butrint History'),('Shkodër History'),('Vlorë History'),('Northern Epirus History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3534,0 FROM concept WHERE preferred_label IN (
 'Welsh Biographies','Welsh Antiquities','Welsh Social & Cultural History',
 'Welsh Ethnography','Glamorgan History','Pembrokeshire History',
 'Snowdonia History','Cardiff History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3544,0 FROM concept WHERE preferred_label IN (
 'Albanian Antiquities','Albanian Social & Cultural History','Albanian Ethnography',
 'Albanian Biographies','Albanian Historiography','Albanian Political History',
 'Albanian Foreign Relations History','Butrint History','Shkodër History',
 'Vlorë History','Northern Epirus History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Albanian Ethnography'),0
FROM concept WHERE preferred_label='Albanians Abroad';
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,10320,0 FROM concept WHERE preferred_label='Post-Communist Albanian Political History';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DR','923','925.Z',concept_id FROM concept WHERE preferred_label='Albanian Ethnography';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DR','928','934.Z',concept_id FROM concept WHERE preferred_label='Albanian Biographies';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DR','935','937.Z',concept_id FROM concept WHERE preferred_label='Albanian Historiography';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DR','949','950.Z',concept_id FROM concept WHERE preferred_label='Albanian Political History';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DR','951','953.Z',concept_id FROM concept WHERE preferred_label='Albanian Foreign Relations History';

CREATE TEMP TABLE wa_selector(selector TEXT,label TEXT);
INSERT INTO wa_selector VALUES
 ('DA710','Welsh Biographies'),('DA711','Welsh Antiquities'),
 ('DA711.5','Welsh Social & Cultural History'),('DA712','Welsh Ethnography'),
 ('DA740.G5','Glamorgan History'),('DA740.P3','Pembrokeshire History'),
 ('DA740.S6','Snowdonia History'),('DA745.C2','Cardiff History'),
 ('DR921','Albanian Antiquities'),('DR922','Albanian Social & Cultural History'),
 ('DR926','Albanians Abroad'),('DR978.3','Post-Communist Albanian Political History'),
 ('DR998.B88','Butrint History'),('DR998.S48','Shkodër History'),
 ('DR998.V44','Vlorë History'),('DR996.E64','Northern Epirus History');
INSERT INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,m.selector FROM wa_selector m JOIN concept c ON c.preferred_label=m.label;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2461,'DA735'),(14011,'DR909');

DROP TABLE wa_selector;
COMMIT;
