-- Short curated display labels for the fifth longest-name batch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Optometry Trade Catalogs' WHERE preferred_label='Catalogs of optometrists'' supplies and other trade publications';
UPDATE concept SET preferred_label='Oral Diseases & Stomatology' WHERE preferred_label='Diseases of the mouth, tongue, and salivary glands. Stomatology';
UPDATE concept SET preferred_label='Malta Before 870' WHERE preferred_label='Early to 870. Phoenician, Greek, Roman and Byzantine domination';
UPDATE concept SET preferred_label='Diplomatic & Foreign Relations' WHERE preferred_label='Political and diplomatic history. Foreign and general relations';
UPDATE concept SET preferred_label='Literature & Cultural History' WHERE preferred_label='Relations to history, civilization, cultures, etc. (in general)';
UPDATE concept SET preferred_label='Wartime Relief & Refugees' WHERE preferred_label='Relief work. Charities. Protection. Refugees. Displaced persons';
UPDATE concept SET preferred_label='Interpretation of Passages' WHERE preferred_label='Textual. Interpretation of detached passages (poetry and prose)';
UPDATE concept SET preferred_label='Enlightenment Philosophy' WHERE preferred_label='18th century. Philosophy of the enlightenment. Die Aufklärung';
UPDATE concept SET preferred_label='Applied Electro-Optics' WHERE preferred_label='Applied electrooptics. Electrooptical devices. Optoelectronics';
UPDATE concept SET preferred_label='Biomedical Engineering Careers' WHERE preferred_label='Biomedical engineering as a profession. Biomedical technicians';
UPDATE concept SET preferred_label='Urban Planning Collections' WHERE preferred_label='Collections of plans (not limited to a single country or city)';
UPDATE concept SET preferred_label='Respiratory Pharmacology' WHERE preferred_label='Drugs acting on the respiratory system. Pulmonary pharmacology';
UPDATE concept SET preferred_label='Protective Packaging' WHERE preferred_label='Packaging for protection from exposure to harmful environments';
UPDATE concept SET preferred_label='Adaptive Physical Education' WHERE preferred_label='Physical education and training for special classes of persons';
UPDATE concept SET preferred_label='Telekinesis & Psychokinesis' WHERE preferred_label='Physical phenomena of spiritualism. Telekinesis. Psychokinesis';
UPDATE concept SET preferred_label='Non-English Reference Books' WHERE preferred_label='Reference books and best books in languages other than English';
UPDATE concept SET preferred_label='Watercolor Techniques & Materials' WHERE preferred_label='Techniques of watercolor painting. Materials and special media';
UPDATE concept SET preferred_label='Context of European Discovery' WHERE preferred_label='Background factors of the discovery, and resulting conditions';
UPDATE concept SET preferred_label='French Chasseurs' WHERE preferred_label='Chasseurs. "Blue devils." Chasseurs à pied. Chasseurs alpine';
UPDATE concept SET preferred_label='Greek Fable Criticism' WHERE preferred_label='Criticism, interpretation, etc. (confined to the Greek fable)';
UPDATE concept SET preferred_label='Directory Compilation' WHERE preferred_label='Directories. General works on the compilation of directories,';
UPDATE concept SET preferred_label='Locomotive Crew Handbooks' WHERE preferred_label='Handbooks, manuals, etc. for locomotive engineers and firemen';
UPDATE concept SET preferred_label='Infection, Immunity & Germ Theory' WHERE preferred_label='Infection and resistance to infection. Germ theory of disease';
UPDATE concept SET preferred_label='Heat-Based Power & Cogeneration' WHERE preferred_label='Production from heat. Cogeneration of electric power and heat';
UPDATE concept SET preferred_label='Art Collection Cataloging' WHERE preferred_label='Accessioning, cataloging, and classification of works of art';

COMMIT;
