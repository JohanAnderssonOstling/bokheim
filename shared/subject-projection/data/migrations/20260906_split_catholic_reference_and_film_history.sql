-- Two audited high-use leaves. Film remains broad at national-cinema level because
-- its exact Cutters identify many individual countries and should not be guessed into regions.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:catholic-reference:societies','Societies, Institutes & Prelatures'),
 ('bokheim:catholic-reference:councils-museums','Councils, Museums & Exhibitions'),
 ('bokheim:catholic-reference:documents','Documents & Collected Works'),
 ('bokheim:catholic-reference:study','Study & Teaching'),
 ('bokheim:catholic-reference:history','Catholic Church History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:catholic-reference:societies','bokheim:catholic:reference-history-study',0,100),
 ('bokheim:catholic-reference:councils-museums','bokheim:catholic:reference-history-study',0,200),
 ('bokheim:catholic-reference:documents','bokheim:catholic:reference-history-study',0,300),
 ('bokheim:catholic-reference:study','bokheim:catholic:reference-history-study',0,400),
 ('bokheim:catholic-reference:history','bokheim:catholic:reference-history-study',0,500);
DELETE FROM source_selector WHERE concept_id='bokheim:catholic:reference-history-study';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:catholic-reference:societies','lcc','BX800..BX819.3@4',0),
 ('bokheim:catholic-reference:councils-museums','lcc','BX820..BX849.9@4',0),
 ('bokheim:catholic-reference:documents','lcc','BX850..BX894.9@4',0),
 ('bokheim:catholic-reference:study','lcc','BX895..BX939@4',0),
 ('bokheim:catholic-reference:history','lcc','BX940..BX1745@4',0);

INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:film-history:general','General Film History & Criticism'),
 ('bokheim:film-history:national','National & Regional Film Histories'),
 ('bokheim:film-history:biography','Film Biography & Directories'),
 ('bokheim:film-history:filmmakers','Individual Filmmakers');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:film-history:general','unified:fc752935-9d26-51f2-829c-195160e8c691',0,100),
 ('bokheim:film-history:national','unified:fc752935-9d26-51f2-829c-195160e8c691',0,200),
 ('bokheim:film-history:biography','unified:fc752935-9d26-51f2-829c-195160e8c691',0,300),
 ('bokheim:film-history:filmmakers','unified:fc752935-9d26-51f2-829c-195160e8c691',0,400);
UPDATE source_selector SET concept_id='bokheim:film-history:general',ordinal=0 WHERE concept_id='unified:fc752935-9d26-51f2-829c-195160e8c691' AND system_id='bisac';
UPDATE source_selector SET concept_id='bokheim:film-history:general',ordinal=0 WHERE concept_id='unified:fc752935-9d26-51f2-829c-195160e8c691' AND selector='PN1993.5';
UPDATE source_selector SET concept_id='bokheim:film-history:national',ordinal=ordinal-1 WHERE concept_id='unified:fc752935-9d26-51f2-829c-195160e8c691' AND selector GLOB 'PN1993.5.*';
UPDATE source_selector SET concept_id='bokheim:film-history:biography',ordinal=CASE selector WHEN 'PN1998' THEN 0 WHEN 'PN1998.A2' THEN 1 ELSE 2 END
 WHERE concept_id='unified:fc752935-9d26-51f2-829c-195160e8c691' AND selector IN ('PN1998','PN1998.A2','PN1998.A3');
UPDATE source_selector SET concept_id='bokheim:film-history:filmmakers',ordinal=CASE selector WHEN 'PN1998.3' THEN 0 ELSE ordinal END
 WHERE concept_id='unified:fc752935-9d26-51f2-829c-195160e8c691' AND (selector='PN1998.3' OR selector GLOB 'PN1998.3.*');

COMMIT;
