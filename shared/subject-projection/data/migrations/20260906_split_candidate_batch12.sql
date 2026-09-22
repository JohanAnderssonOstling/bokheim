PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 43. Veterinary medicine: broad clinical fields plus exact BISAC animal specialties.
INSERT INTO concept VALUES
 ('bokheim:veterinary:general','Veterinary Medicine—General, History & Practice'),
 ('bokheim:veterinary:public','Veterinary Public Health & Preventive Medicine'),
 ('bokheim:veterinary:organs','Veterinary Medicine by Organ & System'),
 ('bokheim:veterinary:surgery','Veterinary Surgery'),('bokheim:veterinary:pharmacology','Veterinary Pharmacology'),
 ('bokheim:veterinary:equine','Equine Veterinary Medicine'),('bokheim:veterinary:food-animal','Food-Animal Veterinary Medicine'),
 ('bokheim:veterinary:small-animal','Small-Animal Veterinary Medicine'),('bokheim:veterinary:dentistry','Veterinary Dentistry');
INSERT INTO concept_parent VALUES
 ('bokheim:veterinary:general','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,100),
 ('bokheim:veterinary:public','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,200),
 ('bokheim:veterinary:organs','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,300),
 ('bokheim:veterinary:surgery','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,400),
 ('bokheim:veterinary:pharmacology','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,500),
 ('bokheim:veterinary:equine','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,600),
 ('bokheim:veterinary:food-animal','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,700),
 ('bokheim:veterinary:small-animal','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,800),
 ('bokheim:veterinary:dentistry','unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82',0,900);
DELETE FROM source_selector WHERE concept_id='unified:c3ff262e-7ab5-5d4f-a1d3-a80122e2cc82';
INSERT INTO source_selector VALUES
 ('bokheim:veterinary:general','bisac','MED089000',0),('bokheim:veterinary:general','lcc','SF600..SF739.9@3',0),
 ('bokheim:veterinary:public','lcc','SF740..SF780.9@3',0),('bokheim:veterinary:organs','lcc','SF781..SF909@3',0),
 ('bokheim:veterinary:surgery','bisac','MED089050',0),('bokheim:veterinary:surgery','lcc','SF910..SF914.9@4',0),
 ('bokheim:veterinary:pharmacology','lcc','SF915..SF950.9@4',0),
 ('bokheim:veterinary:equine','bisac','MED089010',0),('bokheim:veterinary:equine','lcc','SF951..SF959@4',0),
 ('bokheim:veterinary:food-animal','bisac','MED089020',0),('bokheim:veterinary:food-animal','lcc','SF960..SF979.9@4',0),
 ('bokheim:veterinary:small-animal','bisac','MED089030',0),('bokheim:veterinary:small-animal','lcc','SF980..SF1100@4',0),
 ('bokheim:veterinary:dentistry','bisac','MED089040',0);

-- 48. Angling.
INSERT INTO concept VALUES
 ('bokheim:angling:general','Angling—General, History & Literature'),('bokheim:angling:equipment','Angling Equipment & Tackle'),
 ('bokheim:angling:methods','Angling Methods & Bait'),('bokheim:angling:places','Angling by Country or Region'),
 ('bokheim:angling:fish','Angling by Kind of Fish');
INSERT INTO concept_parent VALUES
 ('bokheim:angling:general','unified:31e17ac5-dfd0-5fef-891d-ef4b5de43035',0,100),
 ('bokheim:angling:equipment','unified:31e17ac5-dfd0-5fef-891d-ef4b5de43035',0,200),
 ('bokheim:angling:methods','unified:31e17ac5-dfd0-5fef-891d-ef4b5de43035',0,300),
 ('bokheim:angling:places','unified:31e17ac5-dfd0-5fef-891d-ef4b5de43035',0,400),
 ('bokheim:angling:fish','unified:31e17ac5-dfd0-5fef-891d-ef4b5de43035',0,500);
DELETE FROM source_selector WHERE concept_id='unified:31e17ac5-dfd0-5fef-891d-ef4b5de43035';
INSERT INTO source_selector VALUES
 ('bokheim:angling:general','lcc','SH401..SH446.9@4',0),('bokheim:angling:equipment','lcc','SH447..SH454.9@4',0),
 ('bokheim:angling:methods','lcc','SH455..SH460.9@4',0),('bokheim:angling:places','lcc','SH461..SH679.9@4',0),
 ('bokheim:angling:fish','lcc','SH680..SH691@4',0);

-- 52. Income and factor shares.
INSERT INTO concept VALUES
 ('bokheim:factor-income:general','Income & Factor Shares—General'),('bokheim:factor-income:interest','Interest'),
 ('bokheim:factor-income:wages','Wages as a Factor Share'),('bokheim:factor-income:profit','Profit'),
 ('bokheim:factor-income:entrepreneurship','Entrepreneurship, Risk & Uncertainty'),('bokheim:factor-income:rent','Rent'),
 ('bokheim:factor-income:property','Property & Ownership');
INSERT INTO concept_parent VALUES
 ('bokheim:factor-income:general','unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25',0,100),
 ('bokheim:factor-income:interest','unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25',0,200),
 ('bokheim:factor-income:wages','unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25',0,300),
 ('bokheim:factor-income:profit','unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25',0,400),
 ('bokheim:factor-income:entrepreneurship','unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25',0,500),
 ('bokheim:factor-income:rent','unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25',0,600),
 ('bokheim:factor-income:property','unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25',0,700);
DELETE FROM source_selector WHERE concept_id='unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25';
INSERT INTO source_selector VALUES
 ('bokheim:factor-income:general','lcc','HB522..HB534.9@4',0),('bokheim:factor-income:interest','lcc','HB535..HB551@4',0),
 ('bokheim:factor-income:wages','lcc','HB552..HB600.9@4',0),('bokheim:factor-income:profit','lcc','HB601..HB614.9@4',0),
 ('bokheim:factor-income:entrepreneurship','lcc','HB615..HB669.9@4',0),('bokheim:factor-income:rent','lcc','HB670..HB700.9@4',0),
 ('bokheim:factor-income:property','lcc','HB701..HB715@4',0);

COMMIT;
