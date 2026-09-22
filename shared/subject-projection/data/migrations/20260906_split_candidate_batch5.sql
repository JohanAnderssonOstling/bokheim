-- Agriculture, military aviation, archaeology, and statistics candidate splits.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Livestock & Working Animals
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:livestock:working','Working Animals'),('bokheim:livestock:cattle','Cattle & Dairying'),
 ('bokheim:livestock:horses','Horses'),('bokheim:livestock:donkeys','Donkeys & Asses'),
 ('bokheim:livestock:mules','Mules'),('bokheim:livestock:sheep','Sheep'),('bokheim:livestock:goats','Goats'),('bokheim:livestock:swine','Swine');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:livestock:working','bokheim:animal-culture:livestock-working',0,100),('bokheim:livestock:cattle','bokheim:animal-culture:livestock-working',0,200),
 ('bokheim:livestock:horses','bokheim:animal-culture:livestock-working',0,300),('bokheim:livestock:donkeys','bokheim:animal-culture:livestock-working',0,400),
 ('bokheim:livestock:mules','bokheim:animal-culture:livestock-working',0,500),('bokheim:livestock:sheep','bokheim:animal-culture:livestock-working',0,600),
 ('bokheim:livestock:goats','bokheim:animal-culture:livestock-working',0,700),('bokheim:livestock:swine','bokheim:animal-culture:livestock-working',0,800);
DELETE FROM source_selector WHERE concept_id='bokheim:animal-culture:livestock-working';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:livestock:working','lcc','SF170..SF180@3',0),('bokheim:livestock:cattle','lcc','SF191..SF275@3',0),
 ('bokheim:livestock:horses','lcc','SF277..SF360.4@3',0),('bokheim:livestock:donkeys','lcc','SF361..SF361.73@3',0),
 ('bokheim:livestock:mules','lcc','SF362',0),
 ('bokheim:livestock:sheep','lcc','SF371..SF379@3',0),('bokheim:livestock:goats','lcc','SF380..SF389.3@3',0),
 ('bokheim:livestock:swine','lcc','SF391..SF397.83@3',0);

-- Crops & Economic Botany
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:crops:economic-botany','Economic Botany & Crop Science'),('bokheim:crops:tree','Tree Crops'),
 ('bokheim:crops:food-field','Food, Field & Herb Crops'),('bokheim:crops:vegetables','Vegetables'),
 ('bokheim:crops:fruit','Fruit Culture'),('bokheim:crops:berries-grapes-nuts','Berries, Grapes & Nuts'),
 ('bokheim:crops:agricultural-zoology','Agricultural Zoology & Crop Animals');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:crops:economic-botany','bokheim:subject:plant-culture:crops',0,100),('bokheim:crops:tree','bokheim:subject:plant-culture:crops',0,200),
 ('bokheim:crops:food-field','bokheim:subject:plant-culture:crops',0,300),('bokheim:crops:vegetables','bokheim:subject:plant-culture:crops',0,400),
 ('bokheim:crops:fruit','bokheim:subject:plant-culture:crops',0,500),('bokheim:crops:berries-grapes-nuts','bokheim:subject:plant-culture:crops',0,600),
 ('bokheim:crops:agricultural-zoology','bokheim:subject:plant-culture:crops',0,700);
DELETE FROM source_selector WHERE concept_id='bokheim:subject:plant-culture:crops';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:crops:economic-botany','bisac','TEC003030',0),('bokheim:crops:economic-botany','bisac','TEC003080',1),('bokheim:crops:economic-botany','lcc','SB107..SB109@3',0),
 ('bokheim:crops:tree','bisac','GAR024000',0),('bokheim:crops:tree','lcc','SB169..SB172.5@3',0),
 ('bokheim:crops:food-field','bisac','GAR009000',0),('bokheim:crops:food-field','bisac','GAR031000',1),('bokheim:crops:food-field','lcc','SB175..SB317@3',0),
 ('bokheim:crops:vegetables','bisac','GAR025000',0),('bokheim:crops:vegetables','lcc','SB320..SB353.5@3',0),
 ('bokheim:crops:fruit','bisac','GAR005000',0),('bokheim:crops:fruit','lcc','SB354..SB380.9@3',0),
 ('bokheim:crops:berries-grapes-nuts','bisac','TEC003110',0),('bokheim:crops:berries-grapes-nuts','lcc','SB381..SB402@3',0),
 ('bokheim:crops:agricultural-zoology','lcc','SB992..SB998@3',0),
 ('unified:d1842d7f-919b-533b-b2e1-daf257ca89ea','bisac','TEC003060',0);

-- Air & Space Forces
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:air-forces:general','Air Forces—General, History & Regions'),('bokheim:air-forces:training','Training, Research & Drill'),
 ('bokheim:air-forces:operations','Tactics, Defense & Reconnaissance'),('bokheim:air-forces:personnel','Organization & Personnel'),
 ('bokheim:air-forces:equipment','Air-Force Equipment & Supplies'),('bokheim:air-forces:space','Military Astronautics & Space Warfare');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:air-forces:general','bokheim:military-studies:air-space',0,100),('bokheim:air-forces:training','bokheim:military-studies:air-space',0,200),
 ('bokheim:air-forces:operations','bokheim:military-studies:air-space',0,300),('bokheim:air-forces:personnel','bokheim:military-studies:air-space',0,400),
 ('bokheim:air-forces:equipment','bokheim:military-studies:air-space',0,500),('bokheim:air-forces:space','bokheim:military-studies:air-space',0,600);
DELETE FROM source_selector WHERE concept_id='bokheim:military-studies:air-space';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:air-forces:general','lcc','UG622..UG636.9@3',0),('bokheim:air-forces:training','lcc','UG637..UG699.9@3',0),
 ('bokheim:air-forces:operations','lcc','UG700..UG769.9@3',0),('bokheim:air-forces:personnel','lcc','UG770..UG1099.9@3',0),
 ('bokheim:air-forces:equipment','lcc','UG1100..UG1435@3',0),('bokheim:air-forces:space','lcc','UG1500..UG1530@2',0);

-- Archaeology
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:archaeology:general','General & Reference'),('bokheim:archaeology:theory','Theory & Method'),
 ('bokheim:archaeology:history','History of Archaeology'),('bokheim:archaeology:preservation','Preservation, Conservation & Antiquities Policy'),
 ('bokheim:archaeology:objects','Archaeological Objects & Special Subjects'),('bokheim:archaeology:monuments','Monuments, Sites & Material Remains');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:archaeology:general','unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554',0,100),('bokheim:archaeology:theory','unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554',0,200),
 ('bokheim:archaeology:history','unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554',0,300),('bokheim:archaeology:preservation','unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554',0,400),
 ('bokheim:archaeology:objects','unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554',0,500),('bokheim:archaeology:monuments','unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554',0,600);
DELETE FROM source_selector WHERE concept_id='unified:fe5c7c8d-c393-55c2-a092-a72ec82a0554';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:archaeology:general','lcc','CC',0),('bokheim:archaeology:general','lcc','CC1..CC71.9@2',1),('bokheim:archaeology:general','lcc','CC1-960',2),
 ('bokheim:archaeology:theory','lcc','CC72..CC99.9@3',0),('bokheim:archaeology:history','lcc','CC100..CC134.9@3',0),
 ('bokheim:archaeology:preservation','lcc','CC135..CC199.9@3',0),('bokheim:archaeology:objects','lcc','CC200..CC599.9@3',0),
 ('bokheim:archaeology:monuments','lcc','CC600..CC960@3',0);

-- Statistics
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:statistics:general','General Statistics'),('bokheim:statistics:theory','Statistical Theory & Methods'),
 ('bokheim:statistics:services','Statistical Services & Bureaus'),('bokheim:statistics:vital','Vital Statistics & Records'),
 ('bokheim:statistics:universal','Universal Statistical Data'),('bokheim:statistics:regional','Statistics by Region or Country');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:statistics:general','unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8',0,100),('bokheim:statistics:theory','unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8',0,200),
 ('bokheim:statistics:services','unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8',0,300),('bokheim:statistics:vital','unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8',0,400),
 ('bokheim:statistics:universal','unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8',0,500),('bokheim:statistics:regional','unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8',0,600);
DELETE FROM source_selector WHERE concept_id='unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:statistics:general','bisac','SOC027000',0),('bokheim:statistics:general','lcc','HA',0),('bokheim:statistics:general','lcc','HA1..HA28.9@2',1),('bokheim:statistics:general','lcc','HA1-4737',2),
 ('bokheim:statistics:theory','lcc','HA29..HA35.9@3',0),('bokheim:statistics:services','lcc','HA36..HA37.9@3',0),
 ('bokheim:statistics:vital','lcc','HA38..HA153.9@3',0),('bokheim:statistics:universal','lcc','HA154..HA174.9@3',0),
 ('bokheim:statistics:regional','lcc','HA175..HA4737@3',0);

COMMIT;
