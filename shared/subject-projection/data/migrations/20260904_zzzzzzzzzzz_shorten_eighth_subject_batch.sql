-- Short curated display labels for the eighth longest-name batch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Mechanical & Formulaic Composition' WHERE preferred_label='Composition using mechanical devices or prescribed formulas';
UPDATE concept SET preferred_label='Public Health Directories' WHERE preferred_label='Directories of sanitary authorities, boards of health, etc.';
UPDATE concept SET preferred_label='Furnace-Gas & Waste-Heat Engines' WHERE preferred_label='Engines for utilization of furnace and waste gases and heat';
UPDATE concept SET preferred_label='Unrelated Language Surveys' WHERE preferred_label='Groups of unrelated languages. General surveys of languages';
UPDATE concept SET preferred_label='Neuropsychology & Psychophysiology' WHERE preferred_label='Neuropsychology. Physiological psychology. Psychophysiology';
UPDATE concept SET preferred_label='Ovarian Dysfunction & Failure' WHERE preferred_label='Ovarian dysfunction. Anovulation. Premature ovarian failure';
UPDATE concept SET preferred_label='Papal States, Holy See & Vatican' WHERE preferred_label='Papal States (States of the Church). Holy See. Vatican City';
UPDATE concept SET preferred_label='Attitudes Toward Spiritualism' WHERE preferred_label='Special persons: Attitude, belief, relation to spiritualism';
UPDATE concept SET preferred_label='Special Subjects & Religious Art' WHERE preferred_label='Special subjects, characters, persons, religious arts, etc.';
UPDATE concept SET preferred_label='Campus Facilities & Planning' WHERE preferred_label='University and college physical facilities. Campus planning';
UPDATE concept SET preferred_label='Computer Modeling & Simulation' WHERE preferred_label='Electronic data processing. Computer models and simulation';
UPDATE concept SET preferred_label='Cards, Invitations & Book Jackets' WHERE preferred_label='Greeting cards, postcards, invitations, book jackets, etc.';
UPDATE concept SET preferred_label='Holocaust Denial & Its Criticism' WHERE preferred_label='Holocaust denial. Criticism of Holocaust denial literature';
UPDATE concept SET preferred_label='Industrial, Logging & Mining Locomotives' WHERE preferred_label='Locomotives for industrial purposes, logging, mining, etc.';
UPDATE concept SET preferred_label='Physicians & the Public' WHERE preferred_label='Medical personnel and the public. Physician and the public';
UPDATE concept SET preferred_label='Urban Monuments & Ornamental Structures' WHERE preferred_label='Ornamental structures (Monuments, arches, fountains, etc.)';
UPDATE concept SET preferred_label='Electricity Production by Current Type' WHERE preferred_label='Production classified by character of the electric current';
UPDATE concept SET preferred_label='American Literature in Translation' WHERE preferred_label='Translations of American literature into foreign languages';
UPDATE concept SET preferred_label='Polar & Circumpolar Medicine' WHERE preferred_label='Arctic medicine. Antarctic medicine. Circumpolar medicine';
UPDATE concept SET preferred_label='Biochemistry Careers & Guidance' WHERE preferred_label='Biological chemistry as a profession. Vocational guidance';
UPDATE concept SET preferred_label='Printing Calculators & Ready Reckoners' WHERE preferred_label='Calculators, ready reckoners, printers'' arithmetics, etc.';
UPDATE concept SET preferred_label='Carnivals, Sideshows & Human Oddities' WHERE preferred_label='Carnivals. Side shows. Human abnormalities, wonders, etc.';
UPDATE concept SET preferred_label='Gas-Manufacturing Resources' WHERE preferred_label='Coal, oil, shale, and other resources for gas manufacture';
UPDATE concept SET preferred_label='Kindergarten History & Statistics' WHERE preferred_label='History and statistics of kindergarten and infant schools';
UPDATE concept SET preferred_label='International Scholarly Associations' WHERE preferred_label='International associations, congresses, conferences, etc.';
UPDATE concept SET preferred_label='Authors & Literary Scenes in Art' WHERE preferred_label='Literary subjects. Scenes from the works of great authors';
UPDATE concept SET preferred_label='European Government & Administration' WHERE preferred_label='Political institutions and public administration (Europe)';
UPDATE concept SET preferred_label='Preoperative Patient & Room Preparation' WHERE preferred_label='Preliminary preparation of the patient and operating room';
UPDATE concept SET preferred_label='Americana Libraries & Catalogs' WHERE preferred_label='Private libraries, and booksellers'' catalogs of Americana';
UPDATE concept SET preferred_label='Greek Philosophy, Life & Literature' WHERE preferred_label='Relation of Greek philosophy to Greek life and literature';

COMMIT;
