-- Preserve broad Russia matching while exposing frequent, meaningful
-- geographic subdivisions. Cutter-based regions are exact selectors: treating
-- them as numeric ranges would incorrectly make every DK511.* code equivalent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:history:russia:northern-caucasus','Northern Caucasus'),
 ('bokheim:history:russia:tatarstan-kazan','Tatarstan & Kazan Region'),
 ('bokheim:history:russia:volga-region','Volga Region'),
 ('bokheim:history:russia:siberia','Siberia');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:history:russia:northern-caucasus','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10',0,10),
 ('bokheim:history:russia:tatarstan-kazan','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10',0,20),
 ('bokheim:history:russia:volga-region','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10',0,30),
 ('bokheim:history:russia:siberia','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10',0,40);

DELETE FROM source_selector
WHERE concept_id='unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'
  AND system_id='lcc' AND selector='DK751..DK781@4';

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:history:russia:northern-caucasus','lcc','DK511.C2',0),
 ('bokheim:history:russia:tatarstan-kazan','lcc','DK511.T17',0),
 ('bokheim:history:russia:volga-region','lcc','DK511.V65',0),
 ('bokheim:history:russia:siberia','lcc','DK751..DK781@5',0);

COMMIT;
