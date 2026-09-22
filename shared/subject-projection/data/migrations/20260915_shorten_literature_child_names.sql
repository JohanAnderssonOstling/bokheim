-- Shorten literature labels where the parent already supplies the context.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label=trim(
  replace(replace(replace(preferred_label,' Literature',''),' literature',''),' Lit.','')
)
WHERE concept_id IN (
  SELECT child.concept_id
  FROM concept_parent child
  WHERE child.parent_concept_id IN (5585,5586,5587,5588,5589,5590,5591,5727)
)
AND (preferred_label LIKE '% Literature' OR preferred_label LIKE '% literature' OR preferred_label LIKE '% Lit.');

UPDATE concept SET preferred_label='England' WHERE concept_id=4175;
UPDATE concept SET preferred_label='Germany' WHERE concept_id=4189;
UPDATE concept SET preferred_label='Denmark' WHERE concept_id IN (3869,4174);
UPDATE concept SET preferred_label='Netherlands' WHERE concept_id IN (3899,4209);
UPDATE concept SET preferred_label='Finland' WHERE concept_id IN (4183,4482);
UPDATE concept SET preferred_label='Norway' WHERE concept_id IN (4394,4211);
UPDATE concept SET preferred_label='Sweden' WHERE concept_id IN (4473,4230);
UPDATE concept SET preferred_label='Wales' WHERE concept_id=4233;
UPDATE concept SET preferred_label='Austria' WHERE concept_id=4159;
UPDATE concept SET preferred_label='Czech Republic' WHERE concept_id=4173;
UPDATE concept SET preferred_label='France' WHERE concept_id=4184;
UPDATE concept SET preferred_label='Greece' WHERE concept_id=4192;
UPDATE concept SET preferred_label='Hungary' WHERE concept_id=4193;
UPDATE concept SET preferred_label='Ireland' WHERE concept_id=4198;
UPDATE concept SET preferred_label='Italy' WHERE concept_id=4202;
UPDATE concept SET preferred_label='Poland' WHERE concept_id=4214;
UPDATE concept SET preferred_label='Portugal' WHERE concept_id=4215;
UPDATE concept SET preferred_label='Russia' WHERE concept_id=4216;
UPDATE concept SET preferred_label='Scotland' WHERE concept_id=4220;
UPDATE concept SET preferred_label='Spain' WHERE concept_id=4226;
UPDATE concept SET preferred_label='Old Icelandic & Norwegian' WHERE concept_id=4402;
UPDATE concept SET preferred_label='Flemish' WHERE concept_id=4235;

-- German & Related Literatures already supplies the subject context.
UPDATE concept SET preferred_label='Collections' WHERE concept_id=12701;
UPDATE concept SET preferred_label='Studies' WHERE concept_id=4250;
UPDATE concept SET preferred_label='History' WHERE concept_id=4249;
UPDATE concept SET preferred_label='Low German' WHERE concept_id=4251;
UPDATE concept SET preferred_label='Regional' WHERE concept_id=4252;
UPDATE concept SET preferred_label='1860/70–1960' WHERE concept_id=12704;
UPDATE concept SET preferred_label='1961–2000' WHERE concept_id=12705;
UPDATE concept SET preferred_label='2001–' WHERE concept_id=4246;
UPDATE concept SET preferred_label='1500–c. 1700' WHERE concept_id=12703;
UPDATE concept SET preferred_label='1700–c. 1860/70' WHERE concept_id=4245;

COMMIT;
