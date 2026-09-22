-- Four broad Christology leaves following the official BT schedule.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:christology:doctrine-study','Christological Doctrine & Study'),
 ('bokheim:christology:life','Life of Christ'),
 ('bokheim:christology:narrative-liturgy','Christ in Narrative, Literature & Liturgy'),
 ('bokheim:christology:miracles-devotion','Miracles, Apparitions, Relics & Devotion');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:christology:doctrine-study','unified:3c31a4c9-f232-50b1-b2f8-c69b1bdf83f3',0,100),
 ('bokheim:christology:life','unified:3c31a4c9-f232-50b1-b2f8-c69b1bdf83f3',0,200),
 ('bokheim:christology:narrative-liturgy','unified:3c31a4c9-f232-50b1-b2f8-c69b1bdf83f3',0,300),
 ('bokheim:christology:miracles-devotion','unified:3c31a4c9-f232-50b1-b2f8-c69b1bdf83f3',0,400);
DELETE FROM source_selector WHERE concept_id='unified:3c31a4c9-f232-50b1-b2f8-c69b1bdf83f3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:christology:doctrine-study','bisac','REL067040',0),
 ('bokheim:christology:doctrine-study','lcc','BT198..BT295.9@3',0),
 ('bokheim:christology:life','lcc','BT296..BT500@3',0),
 ('bokheim:christology:narrative-liturgy','lcc','BT501..BT579.9@3',0),
 ('bokheim:christology:miracles-devotion','lcc','BT580..BT590@3',0);
COMMIT;
