-- Final mechanically proven non-Law parent ranges: every listed interval is
-- wholly contained by a direct child's range.
BEGIN;

CREATE TEMP TABLE _covered(concept_id INTEGER,start_code TEXT,end_code TEXT,PRIMARY KEY(concept_id,start_code,end_code));
INSERT INTO _covered VALUES
 (203,'TH2531','TH2533'),(203,'TH2541','TH2547'),(429,'TF445','TF453'),
 (2321,'LB2335.86','LB2335.885'),(2323,'LB3045','LB3048'),(3349,'DT253.4','DT253.5'),
 (3400,'F1030.7','F1030.8'),(3439,'F316.2','F316.23'),(3498,'DS881.2','DS881.84'),
 (3500,'DS913.39312','DS913.39319'),(3539,'DB938','DB939'),(3539,'DB984','DB984.4'),
 (3539,'DB988.3','DB988.5'),(3539,'DB993','DB993.2'),(3539,'DB993.5','DB993.7'),
 (3539,'DB994','DB994.2'),(3539,'DB995','DB995.2'),(3539,'DB996','DB997'),
 (3610,'DK4110','DK4115.2'),(3637,'DS79.757','DS79.769'),(3637,'DS79.76352','DS79.765'),
 (3752,'PM8201','PM8298'),(3757,'PK9101','PK9169'),(3758,'PK7021','PK7037'),
 (3758,'PK7040','PK7045'),(3874,'PT7721','PT7762'),(3874,'PT7770','PT7794'),
 (3874,'PT7835','PT7869'),(4239,'PQ151','PQ221'),(4239,'PQ226','PQ317'),
 (4249,'PT500','PT597'),(4249,'PT605','PT709'),(4249,'PT711','PT871'),
 (4399,'PT8460','PT8489'),(4399,'PT8500','PT8534'),(4399,'PT8540','PT8574'),
 (4404,'PT7170','PT7176'),(4404,'PT7177','PT7211'),(4453,'PQ9061','PQ9081'),
 (4453,'PQ9083','PQ9095'),(4470,'PQ6265','PQ6269'),(5053,'QL461','QL599.82'),
 (5098,'HB77','HB87'),(5233,'HD2331','HD2336.35'),(5233,'HD2337','HD2339'),
 (5233,'HD2358','HD2360.5'),(5250,'HQ101','HQ440.7'),(5250,'HQ31','HQ64'),
 (5250,'HQ444','HQ445'),(5250,'HQ71','HQ72'),(5691,'JL3100.2','JL3102.2'),
 (5691,'JL480','JL489');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r JOIN _covered x USING(concept_id,start_code,end_code);

DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (SELECT concept_id,start_code,end_code FROM _covered);

DROP TABLE _covered;
COMMIT;
