BEGIN;

-- Format 8 migration: preserve each literal broad notation exactly and remove
-- permissive coverage from the non-leaf parent.
DELETE FROM lcc_range WHERE concept_id IN (12253,12281,5237,5147,597);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (12253,'HQ1400-1870.9'),
 (12281,'HQ767.8-792.99'),
 (5237,'HD9000-9495'),
 (5147,'HG3810-4000'),
 (597,'HT165.5-169.9');

WITH entries(label,parent_id,start_code,end_code) AS (VALUES
 ('U.S. Women',12253,'HQ1402','HQ1439'),
 ('Women in Other Countries',12253,'HQ1451','HQ1870.7'),

 ('Child Rearing',12281,'HQ768','HQ778.7'),
 ('Child Psychology for Parents',12281,'HQ771','HQ772'),
 ('Children with Disabilities',12281,'HQ773.6','HQ773.7'),
 ('Child Care & Day Care',12281,'HQ778.5','HQ778.7'),
 ('Childhood Life & Activities',12281,'HQ781','HQ784'),
 ('Children’s Rights',12281,'HQ789','HQ792.2'),

 ('Food Supply & Produce Trade',5237,'HD9000','HD9019'),
 ('Grain & Hay Trade',5237,'HD9030','HD9049'),
 ('Animal Feed Industry',5237,'HD9052','HD9053'),
 ('Baked Products Industry',5237,'HD9057','HD9058'),
 ('Rice Industry',5237,'HD9066','HD9067'),
 ('Cotton Industry',5237,'HD9070','HD9089'),
 ('Sugar Industry',5237,'HD9100','HD9119'),
 ('Tobacco Industry',5237,'HD9130','HD9149'),
 ('Aromatic Plant Industry',5237,'HD9165','HD9166'),
 ('Tea & Coffee Industries',5237,'HD9195','HD9199'),
 ('Spice Industry',5237,'HD9210','HD9211'),
 ('Vegetable Industry',5237,'HD9220','HD9235'),
 ('Fruit & Nut Industries',5237,'HD9240','HD9259'),
 ('Dairy Industry',5237,'HD9275','HD9283.7'),
 ('Grocery Industry',5237,'HD9320','HD9330'),
 ('Beverage Industry',5237,'HD9348','HD9399'),
 ('Alcoholic Beverage Industry',5237,'HD9350','HD9369'),
 ('Wine Industry',5237,'HD9370','HD9389'),
 ('Distilling Industry',5237,'HD9390','HD9395'),
 ('Animal Products Industry',5237,'HD9410','HD9440'),
 ('Fish & Seafood Industry',5237,'HD9450','HD9469'),
 ('Shellfish Industry',5237,'HD9471','HD9472'),
 ('Agricultural Supply Industries',5237,'HD9475','HD9495'),
 ('Fertilizer Industry',5237,'HD9483','HD9484'),
 ('Fishing Equipment Industry',5237,'HD9488','HD9489'),

 ('History of Foreign Exchange',5147,'HG3811','HG3815'),
 ('Foreign-Exchange Theory',5147,'HG3821','HG3823'),
 ('Currency Crises',5147,'HG3851.3','HG3851.32'),
 ('Foreign Exchange by Region',5147,'HG3861','HG3873'),
 ('International Finance',5147,'HG3879','HG3898'),
 ('Balance of Payments',5147,'HG3882','HG3890'),
 ('Monetary Unions',5147,'HG3894','HG3898.2'),
 ('International Finance by Region',5147,'HG3901','HG4000'),

 ('Planning Research & Education',597,'HT165.5','HT165.53'),
 ('U.S. City Planning',597,'HT167','HT168'),
 ('City Planning by Region',597,'HT167','HT169.54'),
 ('Planned Communities & New Towns',597,'HT169.55','HT169.57'),
 ('Ecovillages',597,'HT169.575','HT169.576'),
 ('Gated Communities',597,'HT169.58','HT169.59'),
 ('Zoning',597,'HT169.6','HT169.9'))
INSERT INTO concept(preferred_label)
SELECT DISTINCT label FROM entries
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=entries.label);

WITH entries(label,parent_id,start_code,end_code) AS (VALUES
 ('U.S. Women',12253,'HQ1402','HQ1439'),('Women in Other Countries',12253,'HQ1451','HQ1870.7'),
 ('Child Rearing',12281,'HQ768','HQ778.7'),('Child Psychology for Parents',12281,'HQ771','HQ772'),('Children with Disabilities',12281,'HQ773.6','HQ773.7'),('Child Care & Day Care',12281,'HQ778.5','HQ778.7'),('Childhood Life & Activities',12281,'HQ781','HQ784'),('Children’s Rights',12281,'HQ789','HQ792.2'),
 ('Food Supply & Produce Trade',5237,'HD9000','HD9019'),('Grain & Hay Trade',5237,'HD9030','HD9049'),('Animal Feed Industry',5237,'HD9052','HD9053'),('Baked Products Industry',5237,'HD9057','HD9058'),('Rice Industry',5237,'HD9066','HD9067'),('Cotton Industry',5237,'HD9070','HD9089'),('Sugar Industry',5237,'HD9100','HD9119'),('Tobacco Industry',5237,'HD9130','HD9149'),('Aromatic Plant Industry',5237,'HD9165','HD9166'),('Tea & Coffee Industries',5237,'HD9195','HD9199'),('Spice Industry',5237,'HD9210','HD9211'),('Vegetable Industry',5237,'HD9220','HD9235'),('Fruit & Nut Industries',5237,'HD9240','HD9259'),('Dairy Industry',5237,'HD9275','HD9283.7'),('Grocery Industry',5237,'HD9320','HD9330'),('Beverage Industry',5237,'HD9348','HD9399'),('Alcoholic Beverage Industry',5237,'HD9350','HD9369'),('Wine Industry',5237,'HD9370','HD9389'),('Distilling Industry',5237,'HD9390','HD9395'),('Animal Products Industry',5237,'HD9410','HD9440'),('Fish & Seafood Industry',5237,'HD9450','HD9469'),('Shellfish Industry',5237,'HD9471','HD9472'),('Agricultural Supply Industries',5237,'HD9475','HD9495'),('Fertilizer Industry',5237,'HD9483','HD9484'),('Fishing Equipment Industry',5237,'HD9488','HD9489'),
 ('History of Foreign Exchange',5147,'HG3811','HG3815'),('Foreign-Exchange Theory',5147,'HG3821','HG3823'),('Currency Crises',5147,'HG3851.3','HG3851.32'),('Foreign Exchange by Region',5147,'HG3861','HG3873'),('International Finance',5147,'HG3879','HG3898'),('Balance of Payments',5147,'HG3882','HG3890'),('Monetary Unions',5147,'HG3894','HG3898.2'),('International Finance by Region',5147,'HG3901','HG4000'),
 ('Planning Research & Education',597,'HT165.5','HT165.53'),('U.S. City Planning',597,'HT167','HT168'),('City Planning by Region',597,'HT167','HT169.54'),('Planned Communities & New Towns',597,'HT169.55','HT169.57'),('Ecovillages',597,'HT169.575','HT169.576'),('Gated Communities',597,'HT169.58','HT169.59'),('Zoning',597,'HT169.6','HT169.9'))
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,e.parent_id,0 FROM entries e JOIN concept c ON c.preferred_label=e.label;

WITH entries(label,start_code,end_code) AS (VALUES
 ('U.S. Women','HQ1402','HQ1439'),('Women in Other Countries','HQ1451','HQ1870.7'),
 ('Child Rearing','HQ768','HQ778.7'),('Child Psychology for Parents','HQ771','HQ772'),('Children with Disabilities','HQ773.6','HQ773.7'),('Child Care & Day Care','HQ778.5','HQ778.7'),('Childhood Life & Activities','HQ781','HQ784'),('Children’s Rights','HQ789','HQ792.2'),
 ('Food Supply & Produce Trade','HD9000','HD9019'),('Grain & Hay Trade','HD9030','HD9049'),('Animal Feed Industry','HD9052','HD9053'),('Baked Products Industry','HD9057','HD9058'),('Rice Industry','HD9066','HD9067'),('Cotton Industry','HD9070','HD9089'),('Sugar Industry','HD9100','HD9119'),('Tobacco Industry','HD9130','HD9149'),('Aromatic Plant Industry','HD9165','HD9166'),('Tea & Coffee Industries','HD9195','HD9199'),('Spice Industry','HD9210','HD9211'),('Vegetable Industry','HD9220','HD9235'),('Fruit & Nut Industries','HD9240','HD9259'),('Dairy Industry','HD9275','HD9283.7'),('Grocery Industry','HD9320','HD9330'),('Beverage Industry','HD9348','HD9399'),('Alcoholic Beverage Industry','HD9350','HD9369'),('Wine Industry','HD9370','HD9389'),('Distilling Industry','HD9390','HD9395'),('Animal Products Industry','HD9410','HD9440'),('Fish & Seafood Industry','HD9450','HD9469'),('Shellfish Industry','HD9471','HD9472'),('Agricultural Supply Industries','HD9475','HD9495'),('Fertilizer Industry','HD9483','HD9484'),('Fishing Equipment Industry','HD9488','HD9489'),
 ('History of Foreign Exchange','HG3811','HG3815'),('Foreign-Exchange Theory','HG3821','HG3823'),('Currency Crises','HG3851.3','HG3851.32'),('Foreign Exchange by Region','HG3861','HG3873'),('International Finance','HG3879','HG3898'),('Balance of Payments','HG3882','HG3890'),('Monetary Unions','HG3894','HG3898.2'),('International Finance by Region','HG3901','HG4000'),
 ('Planning Research & Education','HT165.5','HT165.53'),('U.S. City Planning','HT167','HT168'),('City Planning by Region','HT167','HT169.54'),('Planned Communities & New Towns','HT169.55','HT169.57'),('Ecovillages','HT169.575','HT169.576'),('Gated Communities','HT169.58','HT169.59'),('Zoning','HT169.6','HT169.9'))
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT e.start_code,e.end_code,c.concept_id FROM entries e JOIN concept c ON c.preferred_label=e.label;

COMMIT;
