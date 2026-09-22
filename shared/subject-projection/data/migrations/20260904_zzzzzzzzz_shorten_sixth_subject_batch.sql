-- Short curated display labels for the sixth longest-name batch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Analytical Methods in Physics' WHERE preferred_label='Analytical methods used in the solution of physical problems';
UPDATE concept SET preferred_label='Bombproof & Fallout Shelters' WHERE preferred_label='Bombproof construction. Bombproof shelters. Fallout shelters';
UPDATE concept SET preferred_label='British Central Africa & Federation' WHERE preferred_label='British Central Africa. Federation of Rhodesia and Nyasaland';
UPDATE concept SET preferred_label='Collectors & Private Collections' WHERE preferred_label='Collectors and collecting. Collectibles. Private collections';
UPDATE concept SET preferred_label='Cupping, Acupuncture & Hyperemia' WHERE preferred_label='Cupping. Acupuncture. Artificial hyperemia. Mustard plasters';
UPDATE concept SET preferred_label='Disorders of the Will' WHERE preferred_label='Diseases and disorders of the will. Errors and abnormalities';
UPDATE concept SET preferred_label='Larynx & Trachea Diseases' WHERE preferred_label='Diseases of the larynx, vocal cords, epiglottis, and trachea';
UPDATE concept SET preferred_label='Hepatobiliary & Pancreatic Diseases' WHERE preferred_label='Diseases of the liver, gallbladder, bile ducts, and pancreas';
UPDATE concept SET preferred_label='Periodontal Diseases' WHERE preferred_label='Diseases of the supporting structures of teeth. Periodontics';
UPDATE concept SET preferred_label='Alternative-Fuel Engines' WHERE preferred_label='Engines or motors using other combustible liquids and vapors';
UPDATE concept SET preferred_label='Regional History of Southern Africa' WHERE preferred_label='History of several parts of Southern Africa treated together';
UPDATE concept SET preferred_label='Student Correspondence & Pen Pals' WHERE preferred_label='Interscholastic and international correspondence. "Pen pals"';
UPDATE concept SET preferred_label='Canapés, Hors d’Oeuvres & Salads' WHERE preferred_label='Miscellaneous recipes. Canapes, hors d''oeuvres, salads, etc.';
UPDATE concept SET preferred_label='Origins of Printing' WHERE preferred_label='Origin and antecedents of printing. History of the invention';
UPDATE concept SET preferred_label='Saxhorn & Flugelhorn Family' WHERE preferred_label='Other instruments of the bugle (saxhorn, flügelhorn) family';
UPDATE concept SET preferred_label='Postgraduate & Continuing Medical Education' WHERE preferred_label='Postgraduate medical education. Continuing medical education';
UPDATE concept SET preferred_label='Recreation Leadership & Administration' WHERE preferred_label='Recreation leadership. Administration of recreation services';
UPDATE concept SET preferred_label='Prenatal Influence & Stirpiculture' WHERE preferred_label='Supposed prenatal influence. Prenatal culture. Stirpiculture';
UPDATE concept SET preferred_label='Technical Painting Examination' WHERE preferred_label='Technical examination: Expertising, x-ray, micrography, etc.';
UPDATE concept SET preferred_label='Wireless & Mobile Communications' WHERE preferred_label='Wireless communication systems. Mobile communication systems';
UPDATE concept SET preferred_label='Community Recreation Administration' WHERE preferred_label='Administration of recreation services. Community recreation';
UPDATE concept SET preferred_label='Out-of-Print & Wanted Books' WHERE preferred_label='Catalogs of out-of-print books. Lists of books wanted, etc.';
UPDATE concept SET preferred_label='Church Refugee Work' WHERE preferred_label='Church work with refugees, including the sanctuary movement';
UPDATE concept SET preferred_label='College & Interscholastic Athletics' WHERE preferred_label='College athletics. Intramural and interscholastic athletics';
UPDATE concept SET preferred_label='Metal-Engraved Color Prints' WHERE preferred_label='Color prints (General or produced by metal engraving alone)';

COMMIT;
