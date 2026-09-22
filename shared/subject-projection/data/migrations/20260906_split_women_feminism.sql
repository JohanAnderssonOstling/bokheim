BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Women & Feminism'
WHERE concept_id = 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e';

DELETE FROM source_selector
WHERE concept_id = 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e';

INSERT INTO concept (concept_id, preferred_label)
VALUES
  ('bokheim:women:womens-studies-history', 'Women''s Studies & History'),
  ('bokheim:women:feminism', 'Feminism & Feminist Theory'),
  ('bokheim:women:general-works', 'Women — General Works'),
  ('bokheim:women:discrimination-harassment', 'Sex Discrimination & Harassment'),
  ('bokheim:women:development', 'Women in Development'),
  ('bokheim:women:society-culture', 'Women in Society & Culture'),
  ('bokheim:women:by-region-country', 'Women by Region or Country'),
  ('bokheim:women:organizations', 'Women''s Organizations');

INSERT INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order)
VALUES
  ('bokheim:women:womens-studies-history', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 10),
  ('bokheim:women:feminism', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 20),
  ('bokheim:women:general-works', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 30),
  ('unified:9c777c00-5016-544e-b0e0-a4bc7a31335d', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 1, 40),
  ('bokheim:women:discrimination-harassment', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 50),
  ('bokheim:women:development', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 60),
  ('bokheim:women:society-culture', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 70),
  ('bokheim:women:by-region-country', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 80),
  ('bokheim:women:organizations', 'unified:5179e468-0938-5c8c-9e18-46d15ef5a48e', 0, 90);

DELETE FROM source_selector
WHERE concept_id = 'unified:7e85d4b5-17ef-5543-a69c-46cd5a9b9c9b'
  AND system_id = 'bisac'
  AND selector IN ('SOC010000', 'SOC028000');

INSERT INTO source_selector (concept_id, system_id, selector, ordinal)
VALUES
  ('bokheim:women:womens-studies-history', 'bisac', 'SOC028000', 0),
  ('bokheim:women:womens-studies-history', 'lcc', 'HQ1101..HQ1189.9@1', 0),
  ('bokheim:women:feminism', 'bisac', 'SOC010000', 0),
  ('bokheim:women:feminism', 'lcc', 'HQ1190..HQ1199.9@1', 0),
  ('bokheim:women:general-works', 'lcc', 'HQ1200..HQ1235.9@1', 0),
  ('unified:9c777c00-5016-544e-b0e0-a4bc7a31335d', 'lcc', 'HQ1236..HQ1236.9@1', 0),
  ('bokheim:women:discrimination-harassment', 'lcc', 'HQ1237..HQ1239.9@1', 0),
  ('bokheim:women:development', 'lcc', 'HQ1240..HQ1240.9@1', 0),
  ('bokheim:women:society-culture', 'lcc', 'HQ1241..HQ1399.9@1', 0),
  ('bokheim:women:by-region-country', 'lcc', 'HQ1400..HQ1870.9@1', 0),
  ('bokheim:women:organizations', 'lcc', 'HQ1871..HQ2030.7@1', 0);

COMMIT;
