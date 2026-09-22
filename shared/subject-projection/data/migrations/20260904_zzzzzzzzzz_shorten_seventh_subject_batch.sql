-- Short curated display labels for the seventh longest-name batch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Neuropsychopharmacology' WHERE preferred_label='Drugs acting on the nervous system. Neuropsychopharmacology';
UPDATE concept SET preferred_label='Medical Records Administration' WHERE preferred_label='Medical records departments. Medical records administration';
UPDATE concept SET preferred_label='Hospital Outpatient Care' WHERE preferred_label='Outpatient services. Hospital-based ambulatory medical care';
UPDATE concept SET preferred_label='Plant Communities & Phytosociology' WHERE preferred_label='Vegetative interrelation. Plant communities. Phytosociology';
UPDATE concept SET preferred_label='Hydroelectric Power' WHERE preferred_label='Production from waterpower. Hydroelectric power production';
UPDATE concept SET preferred_label='School & Intramural Athletics' WHERE preferred_label='School athletics. Intramural and interscholastic athletics';
UPDATE concept SET preferred_label='Student Affairs Administration' WHERE preferred_label='Student affairs services. Student personnel administration';
UPDATE concept SET preferred_label='Perioperative Care' WHERE preferred_label='Surgical therapeutics. Preoperative and postoperative care';
UPDATE concept SET preferred_label='Air Routes & Airports' WHERE preferred_label='Airways (Routes). Airports and landing fields. Aerodromes';
UPDATE concept SET preferred_label='Child & Adolescent Psychiatric Nursing' WHERE preferred_label='Child psychiatric nursing. Adolescent psychiatric nursing';
UPDATE concept SET preferred_label='Transnational Historical Events' WHERE preferred_label='Historical events not restricted to one country or period';
UPDATE concept SET preferred_label='Spinal, Cisternal & Ventricular Puncture' WHERE preferred_label='Spinal puncture. Cisternal puncture. Ventricular puncture';
UPDATE concept SET preferred_label='Electronic & Computer Music Composition' WHERE preferred_label='Electronic music composition. Computer music composition';
UPDATE concept SET preferred_label='Forensic & Legal Medicine' WHERE preferred_label='Forensic medicine. Medical jurisprudence. Legal medicine';
UPDATE concept SET preferred_label='Social Medicine & Medical Sociology' WHERE preferred_label='Medicine and society. Social medicine. Medical sociology';
UPDATE concept SET preferred_label='Air & Celestial Navigation' WHERE preferred_label='Air navigation. Astronomy and navigation for air pilots';
UPDATE concept SET preferred_label='Architectural Monument Conservation' WHERE preferred_label='Preservation and restoration of architectural monuments';
UPDATE concept SET preferred_label='Medical Informatics' WHERE preferred_label='Computer applications to medicine. Medical informatics';
UPDATE concept SET preferred_label='Emotional Intelligence & Testing' WHERE preferred_label='Emotional intelligence. Emotional intelligence testing';
UPDATE concept SET preferred_label='Private & Business Aircraft' WHERE preferred_label='Private aircraft. Business aircraft. Company airplanes';
UPDATE concept SET preferred_label='Self, Subsidy & Vanity Publishing' WHERE preferred_label='Self-publishing. Subsidy publishing. Vanity publishing';
UPDATE concept SET preferred_label='Immunopharmacology' WHERE preferred_label='Drugs acting on the immune system. Immunopharmacology';
UPDATE concept SET preferred_label='Information Technology & Systems' WHERE preferred_label='Information technology. Information systems (General)';
UPDATE concept SET preferred_label='Optometry & Opticianry Careers' WHERE preferred_label='Optometry as a profession. Opticianry as a profession';
UPDATE concept SET preferred_label='Experimental Pharmacology & Drug Development' WHERE preferred_label='Research. Experimental pharmacology. Drug development';
UPDATE concept SET preferred_label='Wilderness, Desert & Jungle Survival' WHERE preferred_label='Wilderness survival. Desert survival. Jungle survival';
UPDATE concept SET preferred_label='Forensic Ophthalmology' WHERE preferred_label='Forensic examination of eyes. Forensic ophthalmology';
UPDATE concept SET preferred_label='Water Purification & Treatment' WHERE preferred_label='Water purification. Water treatment and conditioning';
UPDATE concept SET preferred_label='Comparative Human–Animal Psychology' WHERE preferred_label='Comparative psychology. Animal and human psychology';
UPDATE concept SET preferred_label='Satellite Geodesy' WHERE preferred_label='Satellite geodesy. Artificial satellites in geodesy';
UPDATE concept SET preferred_label='Zoonotic Diseases' WHERE preferred_label='Zoonoses. Diseases communicable from animals to man';

-- These labels rely on their displayed parent for their full browse context.
UPDATE concept SET preferred_label='Norwegian Literature' WHERE preferred_label='Theory and principles of the study of Norwegian literature';
UPDATE concept SET preferred_label='Old Norse Literature' WHERE preferred_label='Theory and principles of the study of Old Norse literature';
UPDATE concept SET preferred_label='Hematologic Drugs' WHERE preferred_label='Drugs acting on the blood cells and bloodforming organs';
UPDATE concept SET preferred_label='Plant Color Terminology' WHERE preferred_label='Nomenclature of color as applied to plant description';
UPDATE concept SET preferred_label='Lyric Poetry' WHERE preferred_label='Poetry (including treatises confined to lyric poetry)';
UPDATE concept SET preferred_label='College Reminiscences' WHERE preferred_label='Recollections and reminiscences of college days, etc.';
UPDATE concept SET preferred_label='Medical Services & Red Cross' WHERE preferred_label='Medical and sanitary services. Hospitals. Red Cross';
UPDATE concept SET preferred_label='Royal Insignia & Regalia' WHERE preferred_label='Royalty. Insignia. Regalia, crown and coronets, etc.';
UPDATE concept SET preferred_label='Empirical Verification' WHERE preferred_label='Verification. Empirical verification. Verifiability';
UPDATE concept SET preferred_label='Organ & Tissue Preservation' WHERE preferred_label='Techniques of preservation of organs, tissues, etc.';

COMMIT;
