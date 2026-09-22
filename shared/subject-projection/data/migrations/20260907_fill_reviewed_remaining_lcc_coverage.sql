-- Fill reviewed 2026-09-07 LCC coverage gaps in the curated taxonomy.
-- Validated against all 351,177 local individual entries and 3,135,306 observed LCC strings.
-- Existing assignments, concepts, hierarchy and BISAC selectors are preserved.
BEGIN;

-- German History: German states, provinces and regions; named regional descendants retain their ranges.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DD','801.A','801.Z',3524);

-- General History & Soviet Era: Russian/Soviet general reference before the DK19–29 travel section.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DK','1','18.9',11572);

-- Russian Travel: Russian description and travel, explicit schedule range.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DK','19','29',2493);

-- General History & Soviet Era: General historical studies before military history DK50–59.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DK','36','49',11572);

-- General History & Soviet Era: General political history before foreign relations DK65–69.7.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DK','60','64',11572);

-- Belarusian History: Belarus schedule; existing topic, period and travel assignments retained.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DK','507','507.95',3587);

-- Moldovan History: Moldova schedule; existing topic, period and travel assignments retained.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DK','509','509.95',3594);

-- Armenian History: Republic of Armenia / Armenian SSR schedule; not the separate historical Armenia DS schedule.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DK','680','689.5',10717);

-- Latin American History: Latin America general schedule, including population Cutters at F1419.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','1401','1419',3385);

-- Latin American History: Central America general section, ending before Belize F1441.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','1421','1440',3385);

-- Central American Travel: Central American description and travel, explicit schedule range.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','1431','1433.2',2453);

-- Latin American History: West Indies general and Bermuda sections, ending before Bahamas F1650.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','1601','1640',3385);

-- Latin American History: South America general section, ending before Colombia F2251.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','2201','2239',3385);

-- General Reference Works: Notes and queries, explicit reference schedule range.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('AG','305','313',13727);

-- General Works on Islam: General and general-special works on Islam preceding theology BP166.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('BP','160','165.9',13816);

-- Islam: Islamic controversies, apologetics, welfare and missionary activity preceding interreligious relations BP171.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('BP','167','170.85',4605);

-- Islam & Society: Islam and society, extended through economics and work.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('BP','173.25','173.77',12487);

-- Religious Life & Devotion: Islamic religious life; Sufi descendants retain their more specific assignment.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('BP','188','190',12488);

-- General Reference Works: General-reference curiosities: periodicals and early works; AG243 keeps its existing owner.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('AG','240','241',13727);

-- General Reference Works: Modern curiosities and pictorial general-reference works.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('AG','244','250',13727);

-- Finance: Finance general reference and general works, before money HG201.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('HG','1','199',5142);

-- Lotteries: Lotteries, including historical accounts, clubs and national lotteries; schedule HG6105–6270.9.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('HG','6105','6270.9',4641);

-- Christian Family Life: Christian family life for men, husbands and grandparents.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('BV','4528.2','4528.5',12498);

-- Parenting: Christian parenting, including foster parents, fathers and mothers.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('BV','4529','4529.18',12500);

-- Christian Living Topics: Christian living in relation to other special topics, alphabetic filing range.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('BV','4599.5.A','4599.5.Z',13814);

-- Austrian History: Austrian wars with the Turks, explicit schedule range.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DB','60','64',3537);

-- Czechoslovak History: Czech cities and towns, subject to existing specific descendants and travel exceptions.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DB','2650.A','2650.Z',3538);

-- Andorran History: Andorra general reference and history, preserving period and travel descendants.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DC','921','930',3554);

-- Papal States, Holy See & Vatican: Papal States and Vatican social/history section; travel DG793 remains outside.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DG','795.6','796.7',10562);

-- Yugoslavia & Successor States: Yugoslav general reference, before detailed historical and travel sections.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DR','1202','1216',10171);

-- Yugoslavia & Successor States: Yugoslav regions spanning multiple republics, alphabetic schedule range.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DR','1350.A','1350.Z',10171);

-- Slovenian Historical Studies: Slovenian historical reference and collective biographies before historiography DR1381.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DR','1375.5','1378.5',51950);

-- Bosnian Historical Studies: Bosnian historical reference and collective biographies.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DR','1675.5','1677',51962);

-- North Macedonian Historical Studies: North Macedonian historical reference and collective biographies.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DR','2175.5','2176',51975);

-- Bulgarian History: Early Bulgarian history before 1396, including sources, culture and military history.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('DR','73.7','74.25',3545);

-- South: Southern U.S. general reference and historical works; existing specialized descendants retained.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','206','209.6',3422);

-- South: Southern U.S. regions, with specific regional descendants taking precedence.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','217.A','217.Z',3422);

-- Latin American History: Hispaniola as a whole, before country-specific Haiti and Dominican Republic ranges.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','1900','1911',3385);

-- Latin American History: Guiana as a whole, before the separate Guyana and Suriname schedules.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','2351','2354',3385);

-- Latin American History: South Georgia and South Sandwich Islands, separate from the Falklands.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','3032','3032.5',3385);

-- Midwest: Ohio River and Valley, broader than the state of Ohio.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('F','516','520.5',3420);

-- Other Europe: Irish education reference and history, Irish Republic entries belong in Other Europe, outside the United Kingdom destination.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('LA','669.6','669.63',13712);

-- Other Europe: Russian education within the existing other-European education destination.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('LA','839.2','839.4',13712);

-- English Language: English language processing, variation, social aspects, speech and acquisition.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PE','1074.5','1074.85',3709);

-- Dialects & Regional English: English linguistic geography and general dialect reference, preceding specific dialects.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PE','1700','1704',12191);

-- General Reference Works
INSERT INTO lcc_selector(concept_id,selector) VALUES(13727,'AG600');
-- Judaism
INSERT INTO lcc_selector(concept_id,selector) VALUES(11088,'B157.C65');
-- Islamic Studies
INSERT INTO lcc_selector(concept_id,selector) VALUES(4613,'BP173.2');
-- Christian Theology
INSERT INTO lcc_selector(concept_id,selector) VALUES(4581,'BT93.8');
-- Christian Family Life
INSERT INTO lcc_selector(concept_id,selector) VALUES(12498,'BV4529.2');
-- Christian Living Topics
INSERT INTO lcc_selector(concept_id,selector) VALUES(13814,'BV4579.5');
-- Mysticism
INSERT INTO lcc_selector(concept_id,selector) VALUES(12507,'BV5099');
-- Cotswold Hills
INSERT INTO lcc_selector(concept_id,selector) VALUES(51414,'DA670.C83');
-- Nineteenth Century
INSERT INTO lcc_selector(concept_id,selector) VALUES(11558,'DB82.5');
-- Roman History Reference
INSERT INTO lcc_selector(concept_id,selector) VALUES(13622,'DG202');
-- Italian History
INSERT INTO lcc_selector(concept_id,selector) VALUES(3607,'DG538');
-- Italian Travel
INSERT INTO lcc_selector(concept_id,selector) VALUES(2464,'DG793');
-- Brussels
INSERT INTO lcc_selector(concept_id,selector) VALUES(13752,'DH809.95');
-- Luxembourg History
INSERT INTO lcc_selector(concept_id,selector) VALUES(3578,'DH908');
-- Russian Travel
INSERT INTO lcc_selector(concept_id,selector) VALUES(2493,'DK29.2');
-- General History & Soviet Era
INSERT INTO lcc_selector(concept_id,selector) VALUES(11572,'DK293');
-- Latvian History & Culture
INSERT INTO lcc_selector(concept_id,selector) VALUES(13950,'DK504.738');
-- Lithuanian History & Culture
INSERT INTO lcc_selector(concept_id,selector) VALUES(13954,'DK505.285');
-- Post-Maidan Era, 2014–Present
INSERT INTO lcc_selector(concept_id,selector) VALUES(13962,'DK5531');
-- 1850-1917
INSERT INTO lcc_selector(concept_id,selector) VALUES(11338,'DK908.8617');
-- Regions & Provinces
INSERT INTO lcc_selector(concept_id,selector) VALUES(13942,'DL1171');
-- Danish Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(51758,'DL142.5');
-- Counties, Regions & Islands
INSERT INTO lcc_selector(concept_id,selector) VALUES(13747,'DL269');
-- Culture & Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(14014,'DL30');
-- Culture & Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(14014,'DL31');
-- Culture & Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(14014,'DL33.D8');
-- Culture & Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(14014,'DL33.T48');
-- Reykjavík History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51890,'DL398.R5');
-- Culture & Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(14014,'DL42.5');
-- Norwegian Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(51773,'DL442.5');
-- Culture & Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(13934,'DL642');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.B152');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.B155');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.B157');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.B1574');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.B158');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.G205');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.G206');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.G208');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.V105');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.V107');
-- Other Spanish Local History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13847,'DP302.V109');
-- Balkan History Reference
INSERT INTO lcc_selector(concept_id,selector) VALUES(13911,'DR10.5');
-- Balkan History Reference
INSERT INTO lcc_selector(concept_id,selector) VALUES(13911,'DR11');
-- Communist Era, 1944–1989
INSERT INTO lcc_selector(concept_id,selector) VALUES(11546,'DR267.6');
-- History of Balkan Peninsula
INSERT INTO lcc_selector(concept_id,selector) VALUES(3541,'DR49.7');
-- Bulgarian Ethnography
INSERT INTO lcc_selector(concept_id,selector) VALUES(51729,'DR64.5');
-- Larnaca History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51942,'DS54.95.L37');
-- Paphos History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51943,'DS54.95.N47');
-- Nicosia History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51941,'DS54.95.N53');
-- Northern Cyprus History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51940,'DS54.95.N67');
-- Paphos History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51943,'DS54.95.O43');
-- U.S. History
INSERT INTO lcc_selector(concept_id,selector) VALUES(3402,'E177');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.A26');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.A67');
-- South Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51244,'E184.A77');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.A8');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.A94');
-- Portuguese American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51258,'E184.A95');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.B12');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.B2');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.B674');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.B675');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.B676');
-- French American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51245,'E184.B69');
-- Southeast Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(52188,'E184.B87');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.C17');
-- Spanish American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50875,'E184.C22');
-- Spanish American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50875,'E184.C29');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.C36');
-- British American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51240,'E184.C6');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.C8');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.C87');
-- Greek American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51166,'E184.C9');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.C98');
-- Arab American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51159,'E184.D78');
-- Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50882,'E184.E15');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.G24');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.G255');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.G44');
-- West Indian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51266,'E184.G75');
-- African American & Black
INSERT INTO lcc_selector(concept_id,selector) VALUES(3378,'E184.G85');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.G86');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.H3');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.H97');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.I3');
-- Southeast Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(52188,'E184.I45');
-- Arab American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51159,'E184.I55');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.I96');
-- Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50882,'E184.K3');
-- South Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51244,'E184.K35');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.K37');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.K43');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.K87');
-- Southeast Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(52188,'E184.L27');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.L55');
-- South Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51244,'E184.L87');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.L88');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.M33');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.M34');
-- South Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51244,'E184.M37');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.M52');
-- Spanish American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50875,'E184.M53');
-- Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50882,'E184.M65');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.M66');
-- Czech American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51243,'E184.M7');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.M83');
-- Central American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(52180,'E184.N53');
-- Southeast Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(52188,'E184.N93');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.P25');
-- South Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51244,'E184.P36');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.P47');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.R93');
-- Japanese American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50881,'E184.R97');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.S16');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.S17');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.S48');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.S53');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.S54');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.S6');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.S68');
-- German American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50879,'E184.S78');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.T66');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.V46');
-- European American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51252,'E184.W35');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.W5');
-- Other Population Groups
INSERT INTO lcc_selector(concept_id,selector) VALUES(13702,'E184.W58');
-- Southeast Asian American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(52188,'E184.Y36');
-- Arab American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51159,'E184.Y44');
-- African Immigrant History
INSERT INTO lcc_selector(concept_id,selector) VALUES(51264,'E184.Y66');
-- James Madison, 1809–1817
INSERT INTO lcc_selector(concept_id,selector) VALUES(51106,'E349');
-- James Madison, 1809–1817
INSERT INTO lcc_selector(concept_id,selector) VALUES(51106,'E370');
-- James Monroe, 1817–1825
INSERT INTO lcc_selector(concept_id,selector) VALUES(51107,'E375');
-- John Quincy Adams, 1825–1829
INSERT INTO lcc_selector(concept_id,selector) VALUES(51108,'E380');
-- Andrew Jackson, 1829–1837
INSERT INTO lcc_selector(concept_id,selector) VALUES(51109,'E383');
-- Andrew Jackson, 1829–1837
INSERT INTO lcc_selector(concept_id,selector) VALUES(51109,'E385');
-- Martin Van Buren, 1837–1841
INSERT INTO lcc_selector(concept_id,selector) VALUES(51110,'E390');
-- John Tyler, 1841–1845
INSERT INTO lcc_selector(concept_id,selector) VALUES(51112,'E400');
-- Polk Administration, 1845–1849
INSERT INTO lcc_selector(concept_id,selector) VALUES(51114,'E420');
-- Millard Fillmore, 1850–1853
INSERT INTO lcc_selector(concept_id,selector) VALUES(51116,'E430');
-- Franklin Pierce, 1853–1857
INSERT INTO lcc_selector(concept_id,selector) VALUES(51117,'E435');
-- James Buchanan, 1857–1861
INSERT INTO lcc_selector(concept_id,selector) VALUES(51118,'E440');
-- Civil War Period
INSERT INTO lcc_selector(concept_id,selector) VALUES(3408,'E635');
-- Civil War Period
INSERT INTO lcc_selector(concept_id,selector) VALUES(3408,'E646.5');
-- Andrew Johnson, 1865–1869
INSERT INTO lcc_selector(concept_id,selector) VALUES(51145,'E670');
-- Ulysses S. Grant, 1869–1877
INSERT INTO lcc_selector(concept_id,selector) VALUES(51146,'E675');
-- Ulysses S. Grant, 1869–1877
INSERT INTO lcc_selector(concept_id,selector) VALUES(51146,'E680');
-- Rutherford B. Hayes, 1877–1881
INSERT INTO lcc_selector(concept_id,selector) VALUES(51147,'E685');
-- Chester A. Arthur, 1881–1885
INSERT INTO lcc_selector(concept_id,selector) VALUES(51149,'E695');
-- Grover Cleveland, 1885–1889
INSERT INTO lcc_selector(concept_id,selector) VALUES(51150,'E700');
-- Benjamin Harrison, 1889–1893
INSERT INTO lcc_selector(concept_id,selector) VALUES(51151,'E705');
-- Grover Cleveland, 1893–1897
INSERT INTO lcc_selector(concept_id,selector) VALUES(51152,'E710');
-- William McKinley, 1897–1901
INSERT INTO lcc_selector(concept_id,selector) VALUES(51153,'E738');
-- George W. Bush, 2001–2009
INSERT INTO lcc_selector(concept_id,selector) VALUES(51082,'E905');
-- George W. Bush, 2001–2009
INSERT INTO lcc_selector(concept_id,selector) VALUES(51082,'E906');
-- Barack Obama, 2009–2017
INSERT INTO lcc_selector(concept_id,selector) VALUES(51083,'E910');
-- Barack Obama, 2009–2017
INSERT INTO lcc_selector(concept_id,selector) VALUES(51083,'E911');
-- Donald Trump, 2017–2021
INSERT INTO lcc_selector(concept_id,selector) VALUES(51084,'E915');
-- Joe Biden, 2021–2025
INSERT INTO lcc_selector(concept_id,selector) VALUES(51085,'E919');
-- Latin American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(3385,'F1741');
-- Maryland Regions & Counties
INSERT INTO lcc_selector(concept_id,selector) VALUES(51195,'F186.9');
-- Latin American History
INSERT INTO lcc_selector(concept_id,selector) VALUES(3385,'F1991');
-- South
INSERT INTO lcc_selector(concept_id,selector) VALUES(3422,'F220.A1');
-- 1951–
INSERT INTO lcc_selector(concept_id,selector) VALUES(50989,'F231.3.A2');
-- Virginia Regions & Counties
INSERT INTO lcc_selector(concept_id,selector) VALUES(51200,'F231.9');
-- Georgia Regions & Counties
INSERT INTO lcc_selector(concept_id,selector) VALUES(51216,'F291.7');
-- Georgia Regions & Counties
INSERT INTO lcc_selector(concept_id,selector) VALUES(51216,'F291.8');
-- Tennessee Regions & Counties
INSERT INTO lcc_selector(concept_id,selector) VALUES(51307,'F442.1');
-- Tennessee Regions & Counties
INSERT INTO lcc_selector(concept_id,selector) VALUES(51307,'F442.2');
-- Tennessee Regions & Counties
INSERT INTO lcc_selector(concept_id,selector) VALUES(51307,'F442.3');
-- Great Plains Regional History
INSERT INTO lcc_selector(concept_id,selector) VALUES(50912,'F598');
-- Alaska Regions & Boroughs
INSERT INTO lcc_selector(concept_id,selector) VALUES(51324,'F951');
-- Economic & Technical Assistance
INSERT INTO lcc_selector(concept_id,selector) VALUES(13611,'HC59.8');
-- Telecommunications
INSERT INTO lcc_selector(concept_id,selector) VALUES(453,'HE9717');
-- Media & Communications
INSERT INTO lcc_selector(concept_id,selector) VALUES(5170,'HE9739');
-- International Relations by Region
INSERT INTO lcc_selector(concept_id,selector) VALUES(12893,'JZ3876');
-- International Relations by Region
INSERT INTO lcc_selector(concept_id,selector) VALUES(12893,'JZ3877');
-- African Education History
INSERT INTO lcc_selector(concept_id,selector) VALUES(13714,'LA2095.M45');
-- Other Europe
INSERT INTO lcc_selector(concept_id,selector) VALUES(13712,'LA789.5');
-- English for Foreign Speakers
INSERT INTO lcc_selector(concept_id,selector) VALUES(12189,'PE1131');
-- Grammar & Syntax
INSERT INTO lcc_selector(concept_id,selector) VALUES(12195,'PE1170');
-- Translation
INSERT INTO lcc_selector(concept_id,selector) VALUES(12200,'PE1499');
-- Pronunciation, Spelling & Phonology
INSERT INTO lcc_selector(concept_id,selector) VALUES(12193,'PE1500');
-- Dialects & Regional English
INSERT INTO lcc_selector(concept_id,selector) VALUES(12191,'PE3604');
-- English Literature
INSERT INTO lcc_selector(concept_id,selector) VALUES(4175,'PR618');

COMMIT;
