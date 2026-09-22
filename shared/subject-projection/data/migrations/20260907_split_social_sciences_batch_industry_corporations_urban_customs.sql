BEGIN;

DELETE FROM lcc_range WHERE concept_id IN (12206,12204,12337,12299,12313);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (12206,'HD9506-9624'),(12204,'HD9680-9714.2'),
 (12337,'HD2709-2930.7'),(12299,'HT101-160.9'),(12313,'GT3400-5090');

CREATE TEMP TABLE _social_split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _social_split VALUES
 ('Metal Industries',12206,'HD9506','HD9539.5'),('Iron & Steel Industries',12206,'HD9510','HD9529'),('Precious-Metal Industries',12206,'HD9535','HD9536'),('Coal Industry',12206,'HD9540','HD9559'),('Petroleum Industry',12206,'HD9560','HD9580'),('Natural-Gas Industry',12206,'HD9581','HD9581.2'),('Clay & Ceramic Industries',12206,'HD9590','HD9600'),('Pottery & Porcelain Industries',12206,'HD9610','HD9620'),('Stone Industry',12206,'HD9621','HD9621.8'),('Glass Industry',12206,'HD9623','HD9624'),
 ('Heating & Ventilation Industry',12204,'HD9683','HD9683.5'),('Computer Industry',12204,'HD9696.2','HD9696.82'),('Computer-Peripheral Industry',12204,'HD9696.25','HD9696.36'),('Computer Services Industry',12204,'HD9696.67','HD9696.782'),('Nuclear Industry',12204,'HD9698','HD9698.5'),('Machinery Industry',12204,'HD9705','HD9705.5'),('Instrument Industry',12204,'HD9706','HD9706.8'),('Optical Industry',12204,'HD9707','HD9707.5'),('Photographic Equipment Industry',12204,'HD9708','HD9708.5'),('Transportation Equipment Industry',12204,'HD9709','HD9714.2'),('Automobile Industry',12204,'HD9710','HD9710.37'),('Aircraft Industry',12204,'HD9711','HD9711.2'),
 ('Corporate Governance',12337,'HD2741','HD2749'),('Corporate Diversification',12337,'HD2756','HD2756.2'),('Industrial Concentration',12337,'HD2757','HD2768'),('Public Utilities',12337,'HD2763','HD2768'),('U.S. Corporations',12337,'HD2771','HD2798'),('Corporations by Country',12337,'HD2807','HD2930.9'),
 ('History of Urban Sociology',12299,'HT111','HT150'),('North American Urban Sociology',12299,'HT122','HT129'),('U.S. Urban Sociology',12299,'HT123','HT123.52'),('Latin American Urban Sociology',12299,'HT127.5','HT129'),('European Urban Sociology',12299,'HT131','HT145'),
 ('Special Events',12313,'GT3403','GT3406'),('Hospitality & Entertaining',12313,'GT3410','GT3415'),('Town Life',12313,'GT3420','GT3452'),('Country Life',12313,'GT3470','GT3471'),('Island Life',12313,'GT3480','GT3481'),('Court & Castle Life',12313,'GT3510','GT3670.5'),('Chivalric Customs',12313,'GT3600','GT3660'),('Inns, Taverns & Hotels',12313,'GT3770','GT3896'),('Signs & Signboards',12313,'GT3910','GT3911.995'),('Graffiti',12313,'GT3912','GT3913.95'),('Festivals & Holidays',12313,'GT3925','GT4995'),('Processions & Parades',12313,'GT3980','GT4096'),('Carnivals',12313,'GT4180','GT4296'),('Harvest Festivals',12313,'GT4380','GT4496'),('Fairs',12313,'GT4580','GT4696'),('New Year Customs',12313,'GT4905','GT4908'),('Lenten Customs',12313,'GT4928','GT4930'),('Easter Customs',12313,'GT4935','GT4935.5'),('Christmas Customs',12313,'GT4985','GT4992'),('Royal & State Ceremonies',12313,'GT5010','GT5090'),('Coronations',12313,'GT5050','GT5051');

INSERT INTO concept(preferred_label)
SELECT DISTINCT s.label FROM _social_split s
WHERE NOT EXISTS (SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,0 FROM _social_split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _social_split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _social_split;

COMMIT;
