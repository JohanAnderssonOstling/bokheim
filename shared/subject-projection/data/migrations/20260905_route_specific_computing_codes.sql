-- Route specific QA75.5-QA76.95 codes to their narrowest supported curated subjects.
-- Evidence: local LCC MDSConnect 2016 captions and OpenLibrary code frequencies.
-- Literal broad classification strings are exact selectors, not permissive ranges.
-- Unidentified language/hardware cutters stay on the corresponding known parent.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:0193793df4c93bbea2da','Automated Theorem Proving');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:0193793df4c93bbea2da','unified:60797396-c27e-58ca-8f54-bf034abb25e4',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:faa6619f6a8a0ff5cac0','Intelligent Agents');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:faa6619f6a8a0ff5cac0','unified:60797396-c27e-58ca-8f54-bf034abb25e4',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:58530cebaf43ce09706d','Data Mining');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:58530cebaf43ce09706d','unified:33b57a24-037b-5519-b4f4-b444634f256f',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:ce7059d04155c3fa713b','Molecular & DNA Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:ce7059d04155c3fa713b','bokheim:subject:computers:ai-data',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:c1741636a3423d92cdba','Natural Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:c1741636a3423d92cdba','bokheim:subject:computers:ai-data',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:5251f4ed64e28fad61f3','Semantic Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:5251f4ed64e28fad61f3','bokheim:subject:computers:ai-data',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:889a6e404b44181b733c','IT Auditing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:889a6e404b44181b733c','bokheim:subject:computers:applications-management',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:8c3062ab39c0d7e7b048','Integrated Software');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:8c3062ab39c0d7e7b048','bokheim:subject:computers:applications-management',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:72369ef58a443443a4b3','Text Processing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:72369ef58a443443a4b3','bokheim:subject:computers:applications-management',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:36e573dda21f8a6c1cb8','Data Compression');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:36e573dda21f8a6c1cb8','bokheim:subject:computers:foundations',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:3ac02dc3a3454198e7b9','Mathematics of Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:3ac02dc3a3454198e7b9','bokheim:subject:computers:foundations',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:bb4cd99a51446aa62815','Philosophy of Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:bb4cd99a51446aa62815','bokheim:subject:computers:foundations',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:49052d79b7faa5eb3240','Computing Biography');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:49052d79b7faa5eb3240','bokheim:subject:computers:learning-society',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:5d182653e99fb458f5c6','Computing Careers');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:5d182653e99fb458f5c6','bokheim:subject:computers:learning-society',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:1b6b2c8b5238adc98df3','Computing Education & Research');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:1b6b2c8b5238adc98df3','bokheim:subject:computers:learning-society',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:833cf58159816af3530f','Bulletin Board Systems');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:833cf58159816af3530f','bokheim:subject:computers:networks-internet',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:6f0ddc1e8ee71555fa62','Online Data Processing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:6f0ddc1e8ee71555fa62','bokheim:subject:computers:networks-internet',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:0cd019c5d8d36ebf1b63','Computer Software');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:0cd019c5d8d36ebf1b63','bokheim:subject:computers:programming',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:1788fdea9d3a61b9b1d9','ABAP');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:1788fdea9d3a61b9b1d9','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:a445cc3efc97f591a476','ALGOL');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:a445cc3efc97f591a476','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:ca7518bf483830a62b41','APL');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:ca7518bf483830a62b41','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:c2c6f099be0abde784b3','Ada');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:c2c6f099be0abde784b3','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:794baf0f2de67b27fa65','AppleScript');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:794baf0f2de67b27fa65','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:36967f59ff69f5847ffd','BASIC');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:36967f59ff69f5847ffd','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:dc8f6d7b0713d5fe3c6b','C#');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:dc8f6d7b0713d5fe3c6b','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:c042f11f9327fc3c1b9b','C++');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:c042f11f9327fc3c1b9b','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:6015544112cc00b66e52','COBOL');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:6015544112cc00b66e52','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:652010f06ecbb8185575','Fortran');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:652010f06ecbb8185575','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:8975f8ac9218026f972e','Go');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:8975f8ac9218026f972e','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:a3671a89f304bfe5a76c','Haskell');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:a3671a89f304bfe5a76c','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:15556a37e2ebdc391820','Lisp');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:15556a37e2ebdc391820','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:b9d9eab2c99ee3e49d97','Logo');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:b9d9eab2c99ee3e49d97','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:2034fe27add553da9fb0','ML');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:2034fe27add553da9fb0','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:e497e494a20b5e9579b6','Mathematica');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:e497e494a20b5e9579b6','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:fad93b34088454d2e4c7','Objective-C');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:fad93b34088454d2e4c7','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:788d99d9483300805941','PL/1');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:788d99d9483300805941','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:ac0c509d6f1e3697709e','PostScript');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:ac0c509d6f1e3697709e','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:a2dc29bdacdb329310ee','Prolog');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:a2dc29bdacdb329310ee','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:9faee949fca3716ef7f4','R');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:9faee949fca3716ef7f4','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:d2383b5ac4ce001dea24','RPG');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:d2383b5ac4ce001dea24','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:9301a23472a2367fdf8f','SAS');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:9301a23472a2367fdf8f','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:3580acf731049db72485','SPARK');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:3580acf731049db72485','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:ff168864be03b70f6fc7','Scala');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:ff168864be03b70f6fc7','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:3c5838f5b1de18041437','Scheme');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:3c5838f5b1de18041437','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:3e940f6e04be1947a02c','Swift');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:3e940f6e04be1947a02c','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:e38d811ad19ec9f9d605','Tcl');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:e38d811ad19ec9f9d605','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:a77242b45e7c5ab35940','VBScript');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:a77242b45e7c5ab35940','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:48f02e0d5bde5ea4f4cc','XSLT');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:48f02e0d5bde5ea4f4cc','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:ec56e7d1563f29b6cf2d','Z');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:ec56e7d1563f29b6cf2d','unified:a1f43498-7169-52bb-aa41-697293885e9e',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:4d2c9ccb48c2bd34a1db','Constraint Programming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:4d2c9ccb48c2bd34a1db','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:e9243e2d2257951a5b27','Data Structures');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:e9243e2d2257951a5b27','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:134fa828d0ca5bc72557','Declarative Programming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:134fa828d0ca5bc72557','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:056333ed71bf69656ebc','Evolutionary & Genetic Programming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:056333ed71bf69656ebc','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:34de6dc5a98f5af7e5a0','Functional Programming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:34de6dc5a98f5af7e5a0','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:2b12769a799dea4e79a5','Logic Programming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:2b12769a799dea4e79a5','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:bbeb222f3a88893b453d','Microprogramming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:bbeb222f3a88893b453d','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:ea63e44e9308c5960787','Systems Programming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:ea63e44e9308c5960787','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:9e42a30316bf3bc60368','Visual Programming');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:9e42a30316bf3bc60368','unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:f61832143ae2eb991c05','Application Programming Interfaces');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:f61832143ae2eb991c05','unified:be47d643-7f82-5542-9e51-072fcf5cba15',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:696f65a2fe2525d91442','Configuration Management');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:696f65a2fe2525d91442','unified:be47d643-7f82-5542-9e51-072fcf5cba15',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:f352795c113dbd407e19','Debugging');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:f352795c113dbd407e19','unified:be47d643-7f82-5542-9e51-072fcf5cba15',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:7386f0b89daca76de2d5','Formal Methods');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:7386f0b89daca76de2d5','unified:be47d643-7f82-5542-9e51-072fcf5cba15',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:290ebe174c2413136eaa','Software Maintenance');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:290ebe174c2413136eaa','unified:be47d643-7f82-5542-9e51-072fcf5cba15',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:5da8e04abd3f78b376a8','Database Security');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:5da8e04abd3f78b376a8','unified:da206a7f-0a3e-5aad-93fa-bc83e3681be7',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:f505a6944cef73987436','Autonomic Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:f505a6944cef73987436','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:b176285426c486bc27ba','Batch Processing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:b176285426c486bc27ba','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:dcb0a13f9fd272691a7e','Computer Performance');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:dcb0a13f9fd272691a7e','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:d0fbe3f093dbe47884aa','Device Drivers');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:d0fbe3f093dbe47884aa','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:26d2bd33e3ddf3f654bf','Grid Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:26d2bd33e3ddf3f654bf','unified:138f3df6-83cc-507c-aea9-481ab05eb166',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:22612417431e57788303','Fault-Tolerant Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:22612417431e57788303','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:55c69d05f43f03982562','Raspberry Pi');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:55c69d05f43f03982562','unified:34ac2ecb-cbfa-5117-8db3-ed0600585919',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:e153ff19f91543356748','Wearable Computers');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:e153ff19f91543356748','unified:34ac2ecb-cbfa-5117-8db3-ed0600585919',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:195fa0073d80b9352fe2','High-Performance Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:195fa0073d80b9352fe2','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:430b4efb153c98992a42','Hybrid Computers');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:430b4efb153c98992a42','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:004f56bc93a20e067a01','Memory Management');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:004f56bc93a20e067a01','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:00c01a04a4b8aa8a0475','Middleware');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:00c01a04a4b8aa8a0475','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:682645574c39105b0cd3','Mobile Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:682645574c39105b0cd3','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:7d61a42d3ae4f0f89cba','Real-Time Processing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:7d61a42d3ae4f0f89cba','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:970dac385478db9abbe5','System Software');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:970dac385478db9abbe5','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:e0f6c64ad00cd781f7a1','Transaction Processing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:e0f6c64ad00cd781f7a1','bokheim:subject:computers:systems-hardware',0,100);
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES('bokheim:computing-specific:754d503161274bbba781','Ubiquitous Computing');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:computing-specific:754d503161274bbba781','bokheim:subject:computers:systems-hardware',0,100);
DELETE FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
DELETE FROM source_selector WHERE concept_id='bokheim:lcc-detail:745d5e4e-232f-5cfc-af68-ea30fea8c194' AND system_id='lcc' AND selector='QA75..QA99@4';
-- Foundations & Computer Science / Computer Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA75.5';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828','lcc','QA75.5',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
-- Foundations & Computer Science / Computer Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA75.5-76.95';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828','lcc','QA75.5-76.95',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
-- Foundations & Computer Science / Computer Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828','lcc','QA76',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
-- Foundations & Computer Science / Computer Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.15';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828','lcc','QA76.15',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
-- Foundations & Computer Science / Philosophy of Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.167';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:bb4cd99a51446aa62815','lcc','QA76.167',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:bb4cd99a51446aa62815' AND system_id='lcc';
-- Learning, Reference & Society / History
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.17';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:6a824126-4378-5983-adf4-c6fa5b73158a','lcc','QA76.17',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:6a824126-4378-5983-adf4-c6fa5b73158a' AND system_id='lcc';
-- Learning, Reference & Society / Computing Biography
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.2';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:49052d79b7faa5eb3240','lcc','QA76.2',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:49052d79b7faa5eb3240' AND system_id='lcc';
-- Learning, Reference & Society / Computing Biography
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.2.A2';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:49052d79b7faa5eb3240','lcc','QA76.2.A2',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:49052d79b7faa5eb3240' AND system_id='lcc';
-- Learning, Reference & Society / Computing Biography
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.2.J63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:49052d79b7faa5eb3240','lcc','QA76.2.J63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:49052d79b7faa5eb3240' AND system_id='lcc';
-- Foundations & Computer Science / Computer Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.23';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828','lcc','QA76.23',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
-- Foundations & Computer Science / Computer Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.24';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828','lcc','QA76.24',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
-- Learning, Reference & Society / Computing Careers
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.25';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:5d182653e99fb458f5c6','lcc','QA76.25',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:5d182653e99fb458f5c6' AND system_id='lcc';
-- Learning, Reference & Society / Computing Education & Research
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.27';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:1b6b2c8b5238adc98df3','lcc','QA76.27',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:1b6b2c8b5238adc98df3' AND system_id='lcc';
-- Learning, Reference & Society / Computing Education & Research
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.28';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:1b6b2c8b5238adc98df3','lcc','QA76.28',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:1b6b2c8b5238adc98df3' AND system_id='lcc';
-- Learning, Reference & Society / Certification Guides
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:12c108ac-33e3-5106-a513-58c2fb457c0d','lcc','QA76.3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:12c108ac-33e3-5106-a513-58c2fb457c0d' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hybrid Computers
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.38';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:430b4efb153c98992a42','lcc','QA76.38',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:430b4efb153c98992a42' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.5';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.5',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.52';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.52',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Real-Time Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.54';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:7d61a42d3ae4f0f89cba','lcc','QA76.54',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:7d61a42d3ae4f0f89cba' AND system_id='lcc';
-- Systems, Architecture & Hardware / Real-Time Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.545';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:7d61a42d3ae4f0f89cba','lcc','QA76.545',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:7d61a42d3ae4f0f89cba' AND system_id='lcc';
-- Networks & Internet / Online Data Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.55';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:6f0ddc1e8ee71555fa62','lcc','QA76.55',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:6f0ddc1e8ee71555fa62' AND system_id='lcc';
-- Networks & Internet / Online Data Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.57.A43';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:6f0ddc1e8ee71555fa62','lcc','QA76.57.A43',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:6f0ddc1e8ee71555fa62' AND system_id='lcc';
-- Networks & Internet / Online Data Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.57.C65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:6f0ddc1e8ee71555fa62','lcc','QA76.57.C65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:6f0ddc1e8ee71555fa62' AND system_id='lcc';
-- Networks & Internet / Online Data Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.57.M52';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:6f0ddc1e8ee71555fa62','lcc','QA76.57.M52',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:6f0ddc1e8ee71555fa62' AND system_id='lcc';
-- Graphics, Media & User Experience / Interactive & Multimedia
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.575';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:223d0593-1831-5635-a876-23699938950c','lcc','QA76.575',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:223d0593-1831-5635-a876-23699938950c' AND system_id='lcc';
-- Foundations & Computer Science / Parallel Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.58';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:ff092b97-e068-51f2-a55a-7f9d5952264f','lcc','QA76.58',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:ff092b97-e068-51f2-a55a-7f9d5952264f' AND system_id='lcc';
-- Systems, Architecture & Hardware / Distributed Systems / Cloud Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.585';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:d9768334-9a33-55c8-acb1-53e5eb6e8aae','lcc','QA76.585',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:d9768334-9a33-55c8-acb1-53e5eb6e8aae' AND system_id='lcc';
-- Systems, Architecture & Hardware / Mobile Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.59';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:682645574c39105b0cd3','lcc','QA76.59',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:682645574c39105b0cd3' AND system_id='lcc';
-- AI, Data & Emerging Computing / Semantic Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.5913';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:5251f4ed64e28fad61f3','lcc','QA76.5913',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:5251f4ed64e28fad61f3' AND system_id='lcc';
-- Systems, Architecture & Hardware / Ubiquitous Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.5915';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:754d503161274bbba781','lcc','QA76.5915',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:754d503161274bbba781' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware / Wearable Computers
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.592';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:e153ff19f91543356748','lcc','QA76.592',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:e153ff19f91543356748' AND system_id='lcc';
-- Programming & Software Engineering / Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.6';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7','lcc','QA76.6',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7' AND system_id='lcc';
-- Programming & Software Engineering / Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.6-76.66';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7','lcc','QA76.6-76.66',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:817d1954-5200-5be0-b71a-f3b4c7ad27e7' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Constraint Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.612';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:4d2c9ccb48c2bd34a1db','lcc','QA76.612',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:4d2c9ccb48c2bd34a1db' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Declarative Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.615';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:134fa828d0ca5bc72557','lcc','QA76.615',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:134fa828d0ca5bc72557' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Evolutionary & Genetic Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.618';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:056333ed71bf69656ebc','lcc','QA76.618',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:056333ed71bf69656ebc' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Functional Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.62';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:34de6dc5a98f5af7e5a0','lcc','QA76.62',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:34de6dc5a98f5af7e5a0' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Evolutionary & Genetic Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.623';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:056333ed71bf69656ebc','lcc','QA76.623',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:056333ed71bf69656ebc' AND system_id='lcc';
-- Networks & Internet / Internet / Web Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.625';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:53747382-2133-506e-af22-074eba246525','lcc','QA76.625',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:53747382-2133-506e-af22-074eba246525' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Logic Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:2b12769a799dea4e79a5','lcc','QA76.63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:2b12769a799dea4e79a5' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Microprogramming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.635';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:bbeb222f3a88893b453d','lcc','QA76.635',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:bbeb222f3a88893b453d' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Object Oriented
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.64';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eba9d027-8aa6-51eb-ae70-6bed83fe9da1','lcc','QA76.64',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eba9d027-8aa6-51eb-ae70-6bed83fe9da1' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Parallel
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.642';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:8fa3e42b-9826-5ec6-936f-15612d0d9637','lcc','QA76.642',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:8fa3e42b-9826-5ec6-936f-15612d0d9637' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Visual Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:9e42a30316bf3bc60368','lcc','QA76.65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:9e42a30316bf3bc60368' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Systems Programming
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.66';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:ea63e44e9308c5960787','lcc','QA76.66',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:ea63e44e9308c5960787' AND system_id='lcc';
-- Programming & Software Engineering / Languages
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.7';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.7',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.7-76.73';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.7-76.73',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / ABAP
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A12';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:1788fdea9d3a61b9b1d9','lcc','QA76.73.A12',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:1788fdea9d3a61b9b1d9' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A15';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.A15',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A16';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.A16',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / ALGOL
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A24';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:a445cc3efc97f591a476','lcc','QA76.73.A24',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:a445cc3efc97f591a476' AND system_id='lcc';
-- Programming & Software Engineering / Languages / APL
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A27';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:ca7518bf483830a62b41','lcc','QA76.73.A27',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:ca7518bf483830a62b41' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Ada
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:c2c6f099be0abde784b3','lcc','QA76.73.A35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:c2c6f099be0abde784b3' AND system_id='lcc';
-- Programming & Software Engineering / Languages / AppleScript
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A67';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:794baf0f2de67b27fa65','lcc','QA76.73.A67',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:794baf0f2de67b27fa65' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Assembly Language
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.A8';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a87c244e-3cf5-5f30-aa3d-9a52d918064d','lcc','QA76.73.A8',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a87c244e-3cf5-5f30-aa3d-9a52d918064d' AND system_id='lcc';
-- Programming & Software Engineering / Languages / BASIC
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.B3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:36967f59ff69f5847ffd','lcc','QA76.73.B3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:36967f59ff69f5847ffd' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.C';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.C',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / C
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.C15';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:29568c3f-a319-50fe-9d3f-c3103e6bd425','lcc','QA76.73.C15',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:29568c3f-a319-50fe-9d3f-c3103e6bd425' AND system_id='lcc';
-- Programming & Software Engineering / Languages / C++
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.C153';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:c042f11f9327fc3c1b9b','lcc','QA76.73.C153',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:c042f11f9327fc3c1b9b' AND system_id='lcc';
-- Programming & Software Engineering / Languages / C#
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.C154';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:dc8f6d7b0713d5fe3c6b','lcc','QA76.73.C154',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:dc8f6d7b0713d5fe3c6b' AND system_id='lcc';
-- Programming & Software Engineering / Languages / COBOL
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.C25';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:6015544112cc00b66e52','lcc','QA76.73.C25',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:6015544112cc00b66e52' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.F24';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.F24',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Fortran
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.F25';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:652010f06ecbb8185575','lcc','QA76.73.F25',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:652010f06ecbb8185575' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Go
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.G63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:8975f8ac9218026f972e','lcc','QA76.73.G63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:8975f8ac9218026f972e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Haskell
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.H37';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:a3671a89f304bfe5a76c','lcc','QA76.73.H37',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:a3671a89f304bfe5a76c' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.J3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.J3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Java
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.J38';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:d38119fb-8b87-5d84-88e1-8ab64a6e897e','lcc','QA76.73.J38',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:d38119fb-8b87-5d84-88e1-8ab64a6e897e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / JavaScript
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.J39';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:114ef3d5-b3e3-5c42-9cd8-6bd64407db3c','lcc','QA76.73.J39',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:114ef3d5-b3e3-5c42-9cd8-6bd64407db3c' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Lisp
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.L23';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:15556a37e2ebdc391820','lcc','QA76.73.L23',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:15556a37e2ebdc391820' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Logo
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.L63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:b9d9eab2c99ee3e49d97','lcc','QA76.73.L63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:b9d9eab2c99ee3e49d97' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Mathematica
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.M29';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:e497e494a20b5e9579b6','lcc','QA76.73.M29',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:e497e494a20b5e9579b6' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Visual BASIC
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.M53';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:e1dcb299-9a74-54c8-a03c-28fca876b370','lcc','QA76.73.M53',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:e1dcb299-9a74-54c8-a03c-28fca876b370' AND system_id='lcc';
-- Programming & Software Engineering / Languages / ML
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.M6';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:2034fe27add553da9fb0','lcc','QA76.73.M6',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:2034fe27add553da9fb0' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.M63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.M63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Objective-C
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.O115';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:fad93b34088454d2e4c7','lcc','QA76.73.O115',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:fad93b34088454d2e4c7' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P2';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.P2',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P22';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.P22',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / PHP
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P224';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:efc82a9a-39c3-59df-9190-9b801fa65474','lcc','QA76.73.P224',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:efc82a9a-39c3-59df-9190-9b801fa65474' AND system_id='lcc';
-- Programming & Software Engineering / Languages / PL/1
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P25';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:788d99d9483300805941','lcc','QA76.73.P25',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:788d99d9483300805941' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P258';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.P258',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.P35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / PostScript
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P67';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:ac0c509d6f1e3697709e','lcc','QA76.73.P67',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:ac0c509d6f1e3697709e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Prolog
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P76';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:a2dc29bdacdb329310ee','lcc','QA76.73.P76',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:a2dc29bdacdb329310ee' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Python
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.P98';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:bdb105d1-86db-50f7-acdc-f84479840011','lcc','QA76.73.P98',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:bdb105d1-86db-50f7-acdc-f84479840011' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.Q33';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.Q33',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.R24';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.R24',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / RPG
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.R25';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:d2383b5ac4ce001dea24','lcc','QA76.73.R25',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:d2383b5ac4ce001dea24' AND system_id='lcc';
-- Programming & Software Engineering / Languages / R
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.R3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:9faee949fca3716ef7f4','lcc','QA76.73.R3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:9faee949fca3716ef7f4' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Ruby
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.R83';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:23470d68-62c1-5c2d-8463-661dabb619bd','lcc','QA76.73.R83',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:23470d68-62c1-5c2d-8463-661dabb619bd' AND system_id='lcc';
-- Programming & Software Engineering / Languages / SAS
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.S27';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:9301a23472a2367fdf8f','lcc','QA76.73.S27',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:9301a23472a2367fdf8f' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Scala
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.S28';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:ff168864be03b70f6fc7','lcc','QA76.73.S28',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:ff168864be03b70f6fc7' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Scheme
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.S34';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:3c5838f5b1de18041437','lcc','QA76.73.S34',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:3c5838f5b1de18041437' AND system_id='lcc';
-- Programming & Software Engineering / Languages / SPARK
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.S59';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:3580acf731049db72485','lcc','QA76.73.S59',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:3580acf731049db72485' AND system_id='lcc';
-- Programming & Software Engineering / Languages / SQL
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.S67';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:06a4814c-b5ed-5a6f-959f-cf0b4aec41ef','lcc','QA76.73.S67',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:06a4814c-b5ed-5a6f-959f-cf0b4aec41ef' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Swift
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.S95';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:3e940f6e04be1947a02c','lcc','QA76.73.S95',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:3e940f6e04be1947a02c' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Tcl
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.T44';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:e38d811ad19ec9f9d605','lcc','QA76.73.T44',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:e38d811ad19ec9f9d605' AND system_id='lcc';
-- Programming & Software Engineering / Languages / VBScript
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.V27';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:a77242b45e7c5ab35940','lcc','QA76.73.V27',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:a77242b45e7c5ab35940' AND system_id='lcc';
-- Programming & Software Engineering / Languages | Language not identified in local LCC schedule; retain the precise Languages parent instead of guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.W56';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a1f43498-7169-52bb-aa41-697293885e9e','lcc','QA76.73.W56',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a1f43498-7169-52bb-aa41-697293885e9e' AND system_id='lcc';
-- Programming & Software Engineering / Languages / XSLT
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.X58';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:48f02e0d5bde5ea4f4cc','lcc','QA76.73.X58',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:48f02e0d5bde5ea4f4cc' AND system_id='lcc';
-- Programming & Software Engineering / Languages / Z
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.73.Z2';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:ec56e7d1563f29b6cf2d','lcc','QA76.73.Z2',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:ec56e7d1563f29b6cf2d' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.75-76.765';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.75-76.765',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.751';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.751',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.753';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.753',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.754';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.754',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.758';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:be47d643-7f82-5542-9e51-072fcf5cba15','lcc','QA76.758',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:be47d643-7f82-5542-9e51-072fcf5cba15' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.76',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Application Programming Interfaces
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.A63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:f61832143ae2eb991c05','lcc','QA76.76.A63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:f61832143ae2eb991c05' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.A65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.76.A65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Compilers
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.C65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:dbe24969-2551-591b-aa66-7563081dea95','lcc','QA76.76.C65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:dbe24969-2551-591b-aa66-7563081dea95' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.C66';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:be47d643-7f82-5542-9e51-072fcf5cba15','lcc','QA76.76.C66',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:be47d643-7f82-5542-9e51-072fcf5cba15' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Games
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.C672';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:bccb02f1-d1c0-550d-bb1d-118bdbade847','lcc','QA76.76.C672',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:bccb02f1-d1c0-550d-bb1d-118bdbade847' AND system_id='lcc';
-- Security & Digital Forensics / Security / Viruses & Malware
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.C68';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:f4be8de7-9ceb-520e-91ea-3f6bfa07b035','lcc','QA76.76.C68',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:f4be8de7-9ceb-520e-91ea-3f6bfa07b035' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Configuration Management
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.C69';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:696f65a2fe2525d91442','lcc','QA76.76.C69',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:696f65a2fe2525d91442' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.D47';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:be47d643-7f82-5542-9e51-072fcf5cba15','lcc','QA76.76.D47',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:be47d643-7f82-5542-9e51-072fcf5cba15' AND system_id='lcc';
-- Systems, Architecture & Hardware / Device Drivers
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.D49';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:d0fbe3f093dbe47884aa','lcc','QA76.76.D49',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:d0fbe3f093dbe47884aa' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Quality Assurance & Testing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.E93';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9b2b0fd8-f99e-5792-8677-50daf6febf11','lcc','QA76.76.E93',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9b2b0fd8-f99e-5792-8677-50daf6febf11' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence / Expert Systems
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.E95';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:1842b4d3-9bf8-5d37-958f-b68755f3be40','lcc','QA76.76.E95',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:1842b4d3-9bf8-5d37-958f-b68755f3be40' AND system_id='lcc';
-- Graphics, Media & User Experience / Human-Computer Interaction
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.H85';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:ea44fdad-3d18-588f-9eda-d1abfecdf638','lcc','QA76.76.H85',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:ea44fdad-3d18-588f-9eda-d1abfecdf638' AND system_id='lcc';
-- Graphics, Media & User Experience / Interactive & Multimedia
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.H94';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:223d0593-1831-5635-a876-23699938950c','lcc','QA76.76.H94',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:223d0593-1831-5635-a876-23699938950c' AND system_id='lcc';
-- Applications, Business & IT Management / Integrated Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.I57';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:8c3062ab39c0d7e7b048','lcc','QA76.76.I57',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:8c3062ab39c0d7e7b048' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence / Intelligent Agents
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.I58';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:faa6619f6a8a0ff5cac0','lcc','QA76.76.I58',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:faa6619f6a8a0ff5cac0' AND system_id='lcc';
-- Graphics, Media & User Experience / Interactive & Multimedia
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.I59';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:223d0593-1831-5635-a876-23699938950c','lcc','QA76.76.I59',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:223d0593-1831-5635-a876-23699938950c' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software | Software topic not identified in local schedule; do not infer Java from the cutter alone.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.J38';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.76.J38',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Languages / .NET
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.M52';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:72e4d8f7-f102-591e-8964-274bdc9c1871','lcc','QA76.76.M52',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:72e4d8f7-f102-591e-8964-274bdc9c1871' AND system_id='lcc';
-- Systems, Architecture & Hardware / Middleware
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.M54';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:00c01a04a4b8aa8a0475','lcc','QA76.76.M54',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:00c01a04a4b8aa8a0475' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Open Source
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.O62';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:7d1ef2f7-132d-562f-8823-3e8d004e85d0','lcc','QA76.76.O62',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:7d1ef2f7-132d-562f-8823-3e8d004e85d0' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.O63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:2abb0f1d-fd2b-553a-a32e-cd748e411770','lcc','QA76.76.O63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:2abb0f1d-fd2b-553a-a32e-cd748e411770' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.P37';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:be47d643-7f82-5542-9e51-072fcf5cba15','lcc','QA76.76.P37',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:be47d643-7f82-5542-9e51-072fcf5cba15' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Quality Assurance & Testing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.Q35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9b2b0fd8-f99e-5792-8677-50daf6febf11','lcc','QA76.76.Q35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9b2b0fd8-f99e-5792-8677-50daf6febf11' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Quality Assurance & Testing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.R44';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9b2b0fd8-f99e-5792-8677-50daf6febf11','lcc','QA76.76.R44',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9b2b0fd8-f99e-5792-8677-50daf6febf11' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.R47';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:be47d643-7f82-5542-9e51-072fcf5cba15','lcc','QA76.76.R47',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:be47d643-7f82-5542-9e51-072fcf5cba15' AND system_id='lcc';
-- Programming & Software Engineering / Computer Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.S46';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0cd019c5d8d36ebf1b63','lcc','QA76.76.S46',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0cd019c5d8d36ebf1b63' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Software Maintenance
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.S64';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:290ebe174c2413136eaa','lcc','QA76.76.S64',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:290ebe174c2413136eaa' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Quality Assurance & Testing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.S65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9b2b0fd8-f99e-5792-8677-50daf6febf11','lcc','QA76.76.S65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9b2b0fd8-f99e-5792-8677-50daf6febf11' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Software
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.S95';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:970dac385478db9abbe5','lcc','QA76.76.S95',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:970dac385478db9abbe5' AND system_id='lcc';
-- Systems, Architecture & Hardware / Transaction Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.T45';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:e0f6c64ad00cd781f7a1','lcc','QA76.76.T45',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:e0f6c64ad00cd781f7a1' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Quality Assurance & Testing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.T48';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9b2b0fd8-f99e-5792-8677-50daf6febf11','lcc','QA76.76.T48',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9b2b0fd8-f99e-5792-8677-50daf6febf11' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Tools
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.T49';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:cf5ec8e0-4df6-5cc5-b051-61459bd2d8c3','lcc','QA76.76.T49',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:cf5ec8e0-4df6-5cc5-b051-61459bd2d8c3' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.U84';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:b1a32e84-a681-5210-8957-b880aefc6ddb','lcc','QA76.76.U84',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:b1a32e84-a681-5210-8957-b880aefc6ddb' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Quality Assurance & Testing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.V47';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9b2b0fd8-f99e-5792-8677-50daf6febf11','lcc','QA76.76.V47',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9b2b0fd8-f99e-5792-8677-50daf6febf11' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / Windows Desktop
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76.W56';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9c809d39-448d-5481-b4b3-ba274f9bcef1','lcc','QA76.76.W56',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9c809d39-448d-5481-b4b3-ba274f9bcef1' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.76O63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:2abb0f1d-fd2b-553a-a32e-cd748e411770','lcc','QA76.76O63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:2abb0f1d-fd2b-553a-a32e-cd748e411770' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.77';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:2abb0f1d-fd2b-553a-a32e-cd748e411770','lcc','QA76.77',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:2abb0f1d-fd2b-553a-a32e-cd748e411770' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:2abb0f1d-fd2b-553a-a32e-cd748e411770','lcc','QA76.774',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:2abb0f1d-fd2b-553a-a32e-cd748e411770' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / Android
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774.A53';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:cce3ee8e-e42d-50b7-aac7-b319846b4695','lcc','QA76.774.A53',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:cce3ee8e-e42d-50b7-aac7-b319846b4695' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / iOS
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774.I67';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:7b2daaf9-c8ed-583e-aeca-eba00b596a63','lcc','QA76.774.I67',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:7b2daaf9-c8ed-583e-aeca-eba00b596a63' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / Linux
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774.L46';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:58b02aca-681f-51e8-9d61-5c6977651559','lcc','QA76.774.L46',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:58b02aca-681f-51e8-9d61-5c6977651559' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / Mac OS
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774.M33';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eb4d3aa5-0f3d-50b8-8283-0b787476a2b7','lcc','QA76.774.M33',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eb4d3aa5-0f3d-50b8-8283-0b787476a2b7' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / Windows Desktop
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774.M434';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9c809d39-448d-5481-b4b3-ba274f9bcef1','lcc','QA76.774.M434',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9c809d39-448d-5481-b4b3-ba274f9bcef1' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / Windows Desktop
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774.M435';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9c809d39-448d-5481-b4b3-ba274f9bcef1','lcc','QA76.774.M435',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9c809d39-448d-5481-b4b3-ba274f9bcef1' AND system_id='lcc';
-- Systems, Architecture & Hardware / Operating Systems / Windows Server
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.774.M55';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:279fc1f0-9e67-58f3-b9d8-860dbcfc4a81','lcc','QA76.774.M55',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:279fc1f0-9e67-58f3-b9d8-860dbcfc4a81' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A177';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A177',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A4';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A4',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A66';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A66',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A662';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A662',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A6623';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A6623',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A8';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A8',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A82';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A82',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.A824';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.A824',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.B35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.B35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.B53';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.B53',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.C64';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.C64',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.C645';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.C645',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.D7';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.D7',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.E38';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.E38',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I1015';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I1015',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware / Mainframes & Minicomputers
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I12';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc02af29-768a-552e-9a3a-507694ce208b','lcc','QA76.8.I12',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc02af29-768a-552e-9a3a-507694ce208b' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I123';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I123',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I25919';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I25919',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I2593';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I2593',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I2594';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I2594',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I25963';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I25963',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I2684';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I2684',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I292';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I292',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I52';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I52',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware / Tablets
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:4dcb9b92-5476-5ba6-a212-61e5dadcaee8','lcc','QA76.8.I63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:4dcb9b92-5476-5ba6-a212-61e5dadcaee8' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware / Cell Phones & Devices
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I64';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:031d120e-4425-51c2-9fe1-10616d922d6f','lcc','QA76.8.I64',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:031d120e-4425-51c2-9fe1-10616d922d6f' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.I863';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.I863',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware / Personal Computers / Macintosh
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.M3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:78b23f57-6e9d-5cbd-a7b9-dd9e9e6b4a89','lcc','QA76.8.M3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:78b23f57-6e9d-5cbd-a7b9-dd9e9e6b4a89' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.M67';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.M67',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.M6895';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.M6895',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.P138';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.P138',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.P2';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.P2',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.P46';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.P46',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.P47';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.P47',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.P67';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.P67',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware / Raspberry Pi
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.R15';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:55c69d05f43f03982562','lcc','QA76.8.R15',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:55c69d05f43f03982562' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.S25';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.S25',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.S625';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.S625',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.S63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.S63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.T133';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.T133',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.T18';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.T18',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.T183';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.T183',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.T48';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.T48',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.V32';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.V32',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.V37';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.V37',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.V5';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.V5',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware | Named computer/system/microprocessor; no reliable narrower device identity in local schedule.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.8.Z54';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:34ac2ecb-cbfa-5117-8db3-ed0600585919','lcc','QA76.8.Z54',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:34ac2ecb-cbfa-5117-8db3-ed0600585919' AND system_id='lcc';
-- AI, Data & Emerging Computing / Data Science / Neural Networks
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.87';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a0137aa1-a4d7-5b34-beb0-c0008f7739cb','lcc','QA76.87',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a0137aa1-a4d7-5b34-beb0-c0008f7739cb' AND system_id='lcc';
-- Systems, Architecture & Hardware / High-Performance Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.88';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:195fa0073d80b9352fe2','lcc','QA76.88',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:195fa0073d80b9352fe2' AND system_id='lcc';
-- AI, Data & Emerging Computing / Molecular & DNA Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.887';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:ce7059d04155c3fa713b','lcc','QA76.887',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:ce7059d04155c3fa713b' AND system_id='lcc';
-- Foundations & Computer Science / Quantum Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.889';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:dacae2ac-af35-5d56-bc8e-009a9eb0fdc8','lcc','QA76.889',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:dacae2ac-af35-5d56-bc8e-009a9eb0fdc8' AND system_id='lcc';
-- Systems, Architecture & Hardware / Hardware / Tablets
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.893';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:4dcb9b92-5476-5ba6-a212-61e5dadcaee8','lcc','QA76.893',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:4dcb9b92-5476-5ba6-a212-61e5dadcaee8' AND system_id='lcc';
-- Computers & Information Technology
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:6f873123-bbb7-5144-beeb-18c8044fcefb','lcc','QA76.9',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:6f873123-bbb7-5144-beeb-18c8044fcefb' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Data Structures
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A23';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:e9243e2d2257951a5b27','lcc','QA76.9.A23',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:e9243e2d2257951a5b27' AND system_id='lcc';
-- Security & Digital Forensics / Security
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A25';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:da206a7f-0a3e-5aad-93fa-bc83e3681be7','lcc','QA76.9.A25',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:da206a7f-0a3e-5aad-93fa-bc83e3681be7' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:60797396-c27e-58ca-8f54-bf034abb25e4','lcc','QA76.9.A3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:60797396-c27e-58ca-8f54-bf034abb25e4' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Algorithms
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A43';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:237c9c1e-bd77-549e-9ca0-c1a047efb5ee','lcc','QA76.9.A43',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:237c9c1e-bd77-549e-9ca0-c1a047efb5ee' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A48';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:60797396-c27e-58ca-8f54-bf034abb25e4','lcc','QA76.9.A48',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:60797396-c27e-58ca-8f54-bf034abb25e4' AND system_id='lcc';
-- Systems, Architecture & Hardware / Computer Architecture
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A73';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:caadb82a-eba6-52c8-8877-6eb272732166','lcc','QA76.9.A73',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:caadb82a-eba6-52c8-8877-6eb272732166' AND system_id='lcc';
-- Applications, Business & IT Management / IT Auditing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A93';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:889a6e404b44181b733c','lcc','QA76.9.A93',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:889a6e404b44181b733c' AND system_id='lcc';
-- Graphics, Media & User Experience / Virtual & Augmented Reality
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A94';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:78d56269-92dd-5210-862a-8a3b6e067404','lcc','QA76.9.A94',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:78d56269-92dd-5210-862a-8a3b6e067404' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence / Automated Theorem Proving
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A96';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:0193793df4c93bbea2da','lcc','QA76.9.A96',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:0193793df4c93bbea2da' AND system_id='lcc';
-- Systems, Architecture & Hardware / Autonomic Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.A97';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:f505a6944cef73987436','lcc','QA76.9.A97',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:f505a6944cef73987436' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration / Backup & Recovery
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.B32';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:6640ed94-6989-5c44-88dd-192eff74268a','lcc','QA76.9.B32',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:6640ed94-6989-5c44-88dd-192eff74268a' AND system_id='lcc';
-- Systems, Architecture & Hardware / Batch Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.B38';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:b176285426c486bc27ba','lcc','QA76.9.B38',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:b176285426c486bc27ba' AND system_id='lcc';
-- AI, Data & Emerging Computing / Data Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.B45';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:33b57a24-037b-5519-b4f4-b444634f256f','lcc','QA76.9.B45',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:33b57a24-037b-5519-b4f4-b444634f256f' AND system_id='lcc';
-- Computers & Information Technology | Topic not identified in local schedule; retain the computing parent without guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.B56';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:6f873123-bbb7-5144-beeb-18c8044fcefb','lcc','QA76.9.B56',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:6f873123-bbb7-5144-beeb-18c8044fcefb' AND system_id='lcc';
-- Networks & Internet / Bulletin Board Systems
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.B84';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:833cf58159816af3530f','lcc','QA76.9.B84',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:833cf58159816af3530f' AND system_id='lcc';
-- Systems, Architecture & Hardware / Distributed Systems / Client-Server Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C55';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:93ae8408-863f-57e0-8eb6-047fc5d1d84b','lcc','QA76.9.C55',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:93ae8408-863f-57e0-8eb6-047fc5d1d84b' AND system_id='lcc';
-- Systems, Architecture & Hardware / Distributed Systems / Grid Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C58';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:26d2bd33e3ddf3f654bf','lcc','QA76.9.C58',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:26d2bd33e3ddf3f654bf' AND system_id='lcc';
-- Foundations & Computer Science / Mathematics of Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C62';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:3ac02dc3a3454198e7b9','lcc','QA76.9.C62',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:3ac02dc3a3454198e7b9' AND system_id='lcc';
-- Systems, Architecture & Hardware / Computer Performance
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:dcb0a13f9fd272691a7e','lcc','QA76.9.C63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:dcb0a13f9fd272691a7e' AND system_id='lcc';
-- Learning, Reference & Society / Computer Literacy
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C64';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:dd5ab810-b87c-5116-b653-9d7358d1dc1d','lcc','QA76.9.C64',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:dd5ab810-b87c-5116-b653-9d7358d1dc1d' AND system_id='lcc';
-- Systems, Architecture & Hardware / Computer Architecture
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C643';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:caadb82a-eba6-52c8-8877-6eb272732166','lcc','QA76.9.C643',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:caadb82a-eba6-52c8-8877-6eb272732166' AND system_id='lcc';
-- Foundations & Computer Science / Computer Simulation
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:7a6513d3-742e-5fb4-b6c8-910ffb257db4','lcc','QA76.9.C65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:7a6513d3-742e-5fb4-b6c8-910ffb257db4' AND system_id='lcc';
-- Learning, Reference & Society / Social Aspects
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C659';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eb2a5872-1a91-553d-87ea-3a449f43a663','lcc','QA76.9.C659',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eb2a5872-1a91-553d-87ea-3a449f43a663' AND system_id='lcc';
-- Learning, Reference & Society / Social Aspects
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.C66';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eb2a5872-1a91-553d-87ea-3a449f43a663','lcc','QA76.9.C66',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eb2a5872-1a91-553d-87ea-3a449f43a663' AND system_id='lcc';
-- AI, Data & Emerging Computing / Data Science / Data Modeling & Design
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D26';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:f858b427-d135-5b72-94fe-b37ff841bd6d','lcc','QA76.9.D26',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:f858b427-d135-5b72-94fe-b37ff841bd6d' AND system_id='lcc';
-- Applications, Business & IT Management / Database Administration & Management
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a640a589-ebc8-5fd6-b963-ba48b0c50c7a','lcc','QA76.9.D3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a640a589-ebc8-5fd6-b963-ba48b0c50c7a' AND system_id='lcc';
-- Security & Digital Forensics / Security / Database Security
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D314';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:5da8e04abd3f78b376a8','lcc','QA76.9.D314',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:5da8e04abd3f78b376a8' AND system_id='lcc';
-- Applications, Business & IT Management / Business & Productivity Software / Databases
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D32';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:4e85d485-11e7-5731-a6c6-a4705db148a8','lcc','QA76.9.D32',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:4e85d485-11e7-5731-a6c6-a4705db148a8' AND system_id='lcc';
-- Foundations & Computer Science / Data Compression
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D33';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:36e573dda21f8a6c1cb8','lcc','QA76.9.D33',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:36e573dda21f8a6c1cb8' AND system_id='lcc';
-- AI, Data & Emerging Computing / Data Science / Data Mining
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D343';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:58530cebaf43ce09706d','lcc','QA76.9.D343',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:58530cebaf43ce09706d' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration / Backup & Recovery
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D348';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:6640ed94-6989-5c44-88dd-192eff74268a','lcc','QA76.9.D348',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:6640ed94-6989-5c44-88dd-192eff74268a' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Data Structures
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:e9243e2d2257951a5b27','lcc','QA76.9.D35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:e9243e2d2257951a5b27' AND system_id='lcc';
-- AI, Data & Emerging Computing / Data Science / Data Warehousing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D37';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:9c7ca59e-0bd9-5276-bb26-8ce026b9b6c1','lcc','QA76.9.D37',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:9c7ca59e-0bd9-5276-bb26-8ce026b9b6c1' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Debugging
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D43';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:f352795c113dbd407e19','lcc','QA76.9.D43',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:f352795c113dbd407e19' AND system_id='lcc';
-- Systems, Architecture & Hardware / Distributed Systems
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D5';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:138f3df6-83cc-507c-aea9-481ab05eb166','lcc','QA76.9.D5',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:138f3df6-83cc-507c-aea9-481ab05eb166' AND system_id='lcc';
-- Programming & Software Engineering / Documentation & Technical Writing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.D6';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eda28521-c5bb-55c0-8ac2-afe9d8e1811d','lcc','QA76.9.D6',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eda28521-c5bb-55c0-8ac2-afe9d8e1811d' AND system_id='lcc';
-- Learning, Reference & Society / Computer Literacy
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.E53';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:dd5ab810-b87c-5116-b653-9d7358d1dc1d','lcc','QA76.9.E53',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:dd5ab810-b87c-5116-b653-9d7358d1dc1d' AND system_id='lcc';
-- Graphics, Media & User Experience / Interactive & Multimedia
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.E57';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:223d0593-1831-5635-a876-23699938950c','lcc','QA76.9.E57',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:223d0593-1831-5635-a876-23699938950c' AND system_id='lcc';
-- Systems, Architecture & Hardware / Computer Performance
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.E94';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:dcb0a13f9fd272691a7e','lcc','QA76.9.E94',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:dcb0a13f9fd272691a7e' AND system_id='lcc';
-- Applications, Business & IT Management / Information Technology
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.E95';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:c37d1f79-240b-5389-8bab-7e04b9a41c86','lcc','QA76.9.E95',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:c37d1f79-240b-5389-8bab-7e04b9a41c86' AND system_id='lcc';
-- Computers & Information Technology | Topic not identified in local schedule; retain the computing parent without guessing.
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.E96';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:6f873123-bbb7-5144-beeb-18c8044fcefb','lcc','QA76.9.E96',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:6f873123-bbb7-5144-beeb-18c8044fcefb' AND system_id='lcc';
-- Systems, Architecture & Hardware / Fault-Tolerant Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.F38';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:22612417431e57788303','lcc','QA76.9.F38',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:22612417431e57788303' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration / Storage & Retrieval
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.F48';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:b026d5ad-7d13-5aff-98fb-e833d55245b9','lcc','QA76.9.F48',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:b026d5ad-7d13-5aff-98fb-e833d55245b9' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration / Storage & Retrieval
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.F5';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:b026d5ad-7d13-5aff-98fb-e833d55245b9','lcc','QA76.9.F5',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:b026d5ad-7d13-5aff-98fb-e833d55245b9' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Formal Methods
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.F67';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:7386f0b89daca76de2d5','lcc','QA76.9.F67',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:7386f0b89daca76de2d5' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.H35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:b1a32e84-a681-5210-8957-b880aefc6ddb','lcc','QA76.9.H35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:b1a32e84-a681-5210-8957-b880aefc6ddb' AND system_id='lcc';
-- Graphics, Media & User Experience / Human-Computer Interaction
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.H85';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:ea44fdad-3d18-588f-9eda-d1abfecdf638','lcc','QA76.9.H85',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:ea44fdad-3d18-588f-9eda-d1abfecdf638' AND system_id='lcc';
-- AI, Data & Emerging Computing / Data Science / Data Visualization
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.I52';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:d0dde251-f74b-5e67-a3e5-8caae3e14980','lcc','QA76.9.I52',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:d0dde251-f74b-5e67-a3e5-8caae3e14980' AND system_id='lcc';
-- Graphics, Media & User Experience / Interactive & Multimedia
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.I58';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:223d0593-1831-5635-a876-23699938950c','lcc','QA76.9.I58',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:223d0593-1831-5635-a876-23699938950c' AND system_id='lcc';
-- Learning, Reference & Society / Computer Literacy
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.K48';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:dd5ab810-b87c-5116-b653-9d7358d1dc1d','lcc','QA76.9.K48',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:dd5ab810-b87c-5116-b653-9d7358d1dc1d' AND system_id='lcc';
-- Foundations & Computer Science / Logic Design
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.L63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:74576b87-1215-5e5c-a5eb-bc366c741ffa','lcc','QA76.9.L63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:74576b87-1215-5e5c-a5eb-bc366c741ffa' AND system_id='lcc';
-- Applications, Business & IT Management / Information Technology
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.M3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:c37d1f79-240b-5389-8bab-7e04b9a41c86','lcc','QA76.9.M3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:c37d1f79-240b-5389-8bab-7e04b9a41c86' AND system_id='lcc';
-- Foundations & Computer Science / Mathematics of Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.M35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:3ac02dc3a3454198e7b9','lcc','QA76.9.M35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:3ac02dc3a3454198e7b9' AND system_id='lcc';
-- Systems, Architecture & Hardware / Memory Management
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.M45';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:004f56bc93a20e067a01','lcc','QA76.9.M45',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:004f56bc93a20e067a01' AND system_id='lcc';
-- Learning, Reference & Society / Social Aspects
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.M65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eb2a5872-1a91-553d-87ea-3a449f43a663','lcc','QA76.9.M65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eb2a5872-1a91-553d-87ea-3a449f43a663' AND system_id='lcc';
-- AI, Data & Emerging Computing / Natural Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.N37';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:c1741636a3423d92cdba','lcc','QA76.9.N37',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:c1741636a3423d92cdba' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence / Natural Language Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.N38';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:058ef76a-c0cd-5d43-9ea1-3b536c3d7f1a','lcc','QA76.9.N38',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:058ef76a-c0cd-5d43-9ea1-3b536c3d7f1a' AND system_id='lcc';
-- Programming & Software Engineering / Programming / Object Oriented
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.O35';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eba9d027-8aa6-51eb-ae70-6bed83fe9da1','lcc','QA76.9.O35',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eba9d027-8aa6-51eb-ae70-6bed83fe9da1' AND system_id='lcc';
-- Learning, Reference & Society / Social Aspects
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.P75';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eb2a5872-1a91-553d-87ea-3a449f43a663','lcc','QA76.9.P75',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eb2a5872-1a91-553d-87ea-3a449f43a663' AND system_id='lcc';
-- AI, Data & Emerging Computing / Data Science / Data Analytics
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.Q36';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:3be1cd74-f7aa-5633-801b-d7d7bc94a03f','lcc','QA76.9.Q36',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:3be1cd74-f7aa-5633-801b-d7d7bc94a03f' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.Q4';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:60797396-c27e-58ca-8f54-bf034abb25e4','lcc','QA76.9.Q4',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:60797396-c27e-58ca-8f54-bf034abb25e4' AND system_id='lcc';
-- AI, Data & Emerging Computing / Artificial Intelligence
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.S63';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:60797396-c27e-58ca-8f54-bf034abb25e4','lcc','QA76.9.S63',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:60797396-c27e-58ca-8f54-bf034abb25e4' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Systems Analysis & Design
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.S84';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:4def3042-93d8-53dc-b739-817d05e6a452','lcc','QA76.9.S84',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:4def3042-93d8-53dc-b739-817d05e6a452' AND system_id='lcc';
-- Programming & Software Engineering / Software Development & Engineering / Systems Analysis & Design
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.S88';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:4def3042-93d8-53dc-b739-817d05e6a452','lcc','QA76.9.S88',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:4def3042-93d8-53dc-b739-817d05e6a452' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.T43';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:b1a32e84-a681-5210-8957-b880aefc6ddb','lcc','QA76.9.T43',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:b1a32e84-a681-5210-8957-b880aefc6ddb' AND system_id='lcc';
-- Applications, Business & IT Management / Text Processing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.T48';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:72369ef58a443443a4b3','lcc','QA76.9.T48',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:72369ef58a443443a4b3' AND system_id='lcc';
-- Graphics, Media & User Experience / User Interfaces
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.U83';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:8b405fde-289f-5b09-8f1c-3ee60513f3fb','lcc','QA76.9.U83',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:8b405fde-289f-5b09-8f1c-3ee60513f3fb' AND system_id='lcc';
-- Systems, Architecture & Hardware / System Administration / Virtualization & Containerization
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.V5';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:daa71ad4-1eea-5702-9148-a196fa1cc0f8','lcc','QA76.9.V5',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:daa71ad4-1eea-5702-9148-a196fa1cc0f8' AND system_id='lcc';
-- Applications, Business & IT Management / Business & Productivity Software / Databases
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.W43';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:4e85d485-11e7-5731-a6c6-a4705db148a8','lcc','QA76.9.W43',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:4e85d485-11e7-5731-a6c6-a4705db148a8' AND system_id='lcc';
-- Learning, Reference & Society / Social Aspects
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9.W65';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:eb2a5872-1a91-553d-87ea-3a449f43a663','lcc','QA76.9.W65',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:eb2a5872-1a91-553d-87ea-3a449f43a663' AND system_id='lcc';
-- Foundations & Computer Science / Mathematics of Computing
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.95';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:computing-specific:3ac02dc3a3454198e7b9','lcc','QA76.95',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:computing-specific:3ac02dc3a3454198e7b9' AND system_id='lcc';
-- Applications, Business & IT Management / Database Administration & Management
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.9D3';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:a640a589-ebc8-5fd6-b963-ba48b0c50c7a','lcc','QA76.9D3',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:a640a589-ebc8-5fd6-b963-ba48b0c50c7a' AND system_id='lcc';
-- Foundations & Computer Science / Computer Science
DELETE FROM source_selector WHERE system_id='lcc' AND selector='QA76.A3eb';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828','lcc','QA76.A3eb',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:lcc-detail:745d5e4e-232f-5cfc-af68-ea30fea8c194','lcc','QA75',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:lcc-detail:745d5e4e-232f-5cfc-af68-ea30fea8c194' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:lcc-detail:745d5e4e-232f-5cfc-af68-ea30fea8c194','lcc','QA77..QA99@4',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:lcc-detail:745d5e4e-232f-5cfc-af68-ea30fea8c194' AND system_id='lcc';
COMMIT;
