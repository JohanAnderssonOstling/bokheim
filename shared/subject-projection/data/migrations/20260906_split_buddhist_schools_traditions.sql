-- Split Buddhist schools into the official BQ divisions. Biography selectors
-- are corrected to the existing history/culture leaf.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:buddhist-schools:general','General & Comparative Traditions'),
 ('bokheim:buddhist-schools:theravada','Theravada Buddhism'),
 ('bokheim:buddhist-schools:mahayana','Mahayana Buddhism'),
 ('bokheim:buddhist-schools:tibetan','Tibetan Buddhism'),
 ('bokheim:buddhist-schools:bon','Bon'),
 ('bokheim:buddhist-schools:zen','Zen Buddhism'),
 ('bokheim:buddhist-schools:other','Other Buddhist Schools & Sects');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:buddhist-schools:general','bokheim:buddhism:schools-traditions',0,100),
 ('bokheim:buddhist-schools:theravada','bokheim:buddhism:schools-traditions',0,200),
 ('bokheim:buddhist-schools:mahayana','bokheim:buddhism:schools-traditions',0,300),
 ('bokheim:buddhist-schools:tibetan','bokheim:buddhism:schools-traditions',0,400),
 ('bokheim:buddhist-schools:bon','bokheim:buddhism:schools-traditions',0,500),
 ('bokheim:buddhist-schools:zen','bokheim:buddhism:schools-traditions',0,600),
 ('bokheim:buddhist-schools:other','bokheim:buddhism:schools-traditions',0,700);
UPDATE source_selector SET concept_id='bokheim:buddhism:history-culture-places',ordinal=98 WHERE concept_id='bokheim:buddhism:schools-traditions' AND selector='BQ860..BQ939@4';
UPDATE source_selector SET concept_id='bokheim:buddhism:history-culture-places',ordinal=99 WHERE concept_id='bokheim:buddhism:schools-traditions' AND selector='BQ940..BQ999@4';
DELETE FROM source_selector WHERE concept_id='bokheim:buddhism:schools-traditions';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:buddhist-schools:general','lcc','BQ7001..BQ7099@4',0),
 ('bokheim:buddhist-schools:theravada','bisac','REL007040',0),
 ('bokheim:buddhist-schools:theravada','lcc','BQ7100..BQ7285@4',0),
 ('bokheim:buddhist-schools:mahayana','lcc','BQ7300..BQ7529.2@4',0),
 ('bokheim:buddhist-schools:tibetan','bisac','REL007050',0),
 ('bokheim:buddhist-schools:tibetan','lcc','BQ7530..BQ7950.2@4',0),
 ('bokheim:buddhist-schools:bon','lcc','BQ7960..BQ7989@4',0),
 ('bokheim:buddhist-schools:zen','bisac','REL092000',0),
 ('bokheim:buddhist-schools:other','lcc','BQ8000..BQ9800@4',0);
COMMIT;
