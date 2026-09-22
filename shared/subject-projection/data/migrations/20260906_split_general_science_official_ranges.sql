-- Replace enumerated Q selectors with the official direct children of Q1-Q390.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE _q_active(ok INTEGER PRIMARY KEY);
INSERT INTO _q_active SELECT 1 WHERE EXISTS (SELECT 1 FROM source_selector WHERE concept_id=4779 AND system_id='lcc');
CREATE TEMP TABLE _q_base(id INTEGER NOT NULL);
INSERT INTO _q_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _q_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,ordinal INTEGER NOT NULL);
INSERT INTO _q_node VALUES
 (1,'Operations research',0),
 (2,'Social aspects',1),
 (3,'Instruments and apparatus (General)',2),
 (4,'Scientific illustration',3),
 (5,'Study and teaching',4),
 (6,'Philosophy. Methodology',5),
 (7,'Scientific ethics',6),
 (8,'Classification of the sciences',7),
 (9,'Miscellany and curiosa',8),
 (10,'Scientific voyages and expeditions (General)',9),
 (11,'Periodicals. By language of publication',10),
 (12,'Yearbooks',11),
 (13,'Collected works',12),
 (14,'Societies',13),
 (15,'Museums. Exhibitions',14),
 (16,'Congresses',15),
 (17,'Encyclopedias',16),
 (18,'Dictionaries',17),
 (19,'Translating services. Translating',18),
 (20,'Science readers in English',19),
 (21,'History',20),
 (22,'Contributions to science. By religious or ethnic groups',21),
 (23,'Women in science. Women scientists',22),
 (24,'Biography',23),
 (25,'Directories',24),
 (26,'Science as a profession. Vocational guidance',25),
 (27,'Early works through 1800',26),
 (28,'General works and treatises',27),
 (29,'Textbooks',28),
 (30,'Pictorial works and atlases',29),
 (31,'Popular works',30),
 (32,'Recreations, home experiments, etc.',31),
 (33,'Anecdotes, facetiae, satire, etc.',32),
 (34,'Addresses, essays, lectures',33),
 (35,'Special aspects of the subject as a whole',34),
 (36,'Special topics, A-Z',35),
 (37,'Periodicity. Cycles',36),
 (38,'Nomenclature, terminology, notation, abbreviations',37),
 (39,'Research',38),
 (40,'Audiovisual aids in science teaching',39),
 (41,'Data processing',40),
 (42,'Handbooks, tables, formulas, etc.',41),
 (43,'Science readers for the study of foreign languages',42),
 (44,'Communication in science',43),
 (46,'Cybernetics',45),
 (47,'Juvenile works',46),
 (48,'Brain drain',47);
CREATE TEMP TABLE _q_selector(seq INTEGER NOT NULL,selector TEXT NOT NULL);
INSERT INTO _q_selector VALUES
 (1,'Q500..Q510@3'),
 (2,'Q175.4..Q175.55@3'),
 (3,'Q184..Q185.7@3'),
 (4,'Q222'),
 (5,'Q181..Q183.4@3'),
 (6,'Q174..Q175.3@3'),
 (7,'Q175.35..Q175.37@3'),
 (8,'Q177'),
 (9,'Q173'),
 (10,'Q115..Q116@3'),
 (11,'Q1..Q4@3'),
 (12,'Q9'),
 (13,'Q111..Q113@3'),
 (14,'Q10..Q99@3'),
 (15,'Q105.A1..Q105.Z@3'),
 (16,'Q101'),
 (17,'Q121'),
 (18,'Q123'),
 (19,'Q124'),
 (20,'Q209'),
 (21,'Q124.6..Q127.2@3'),
 (22,'Q128'),
 (23,'Q130'),
 (24,'Q141..Q143@3'),
 (25,'Q145'),
 (26,'Q147..Q149@3'),
 (27,'Q151..Q157@3'),
 (28,'Q158..Q158.5@3'),
 (29,'Q160..Q161.2@3'),
 (30,'Q161.7'),
 (31,'Q162'),
 (32,'Q164'),
 (33,'Q167'),
 (34,'Q171'),
 (35,'Q172'),
 (36,'Q172.5.A..Q172.5.Z@3'),
 (37,'Q176'),
 (38,'Q179'),
 (39,'Q179.9..Q180.7@3'),
 (40,'Q190..Q197@3'),
 (41,'Q183.9'),
 (42,'Q199'),
 (43,'Q211..Q219@3'),
 (44,'Q223..Q227@3'),
 (46,'Q300..Q390@3'),
 (47,'Q163'),
 (48,'Q150..Q150.2@3');
INSERT INTO concept(concept_id,preferred_label) SELECT b.id+n.seq,n.label FROM _q_node n CROSS JOIN _q_base b JOIN _q_active;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT b.id+n.seq,4779,n.ordinal,n.ordinal FROM _q_node n CROSS JOIN _q_base b JOIN _q_active;
INSERT INTO source_selector(concept_id,system_id,selector) SELECT b.id+s.seq,'lcc',s.selector FROM _q_selector s CROSS JOIN _q_base b JOIN _q_active;
DELETE FROM source_selector WHERE concept_id=4779 AND system_id='lcc' AND EXISTS(SELECT 1 FROM _q_active);
DROP TABLE _q_selector;
DROP TABLE _q_node;
DROP TABLE _q_base;
DROP TABLE _q_active;
COMMIT;
