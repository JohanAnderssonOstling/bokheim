BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||substr(end_code,length(rtrim(end_code,'0123456789.'))+1)
FROM lcc_range WHERE concept_id IN (52470,52732,52435,11884);
DELETE FROM lcc_range WHERE concept_id IN (52470,52732,52435,11884);

CREATE TEMP TABLE _split(label TEXT,parent_id,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('Balance of Payments',52470,'HG3882','HG3890'),('Monetary Unions & Currency Areas',52470,'HG3894','HG3898.2'),
 ('Lesbian Studies',52732,'HQ75.3','HQ75.6'),('Gay Men’s Studies',52732,'HQ75.7','HQ76.2'),('Homophobia & Heterosexism',52732,'HQ76.4','HQ76.45'),('Gay Rights Movement',52732,'HQ76.5','HQ76.8'),('Gay & Lesbian Press',52732,'HQ76.9','HQ76.95'),
 ('Child Psychology for Parents',52435,'HQ771','HQ772'),('Children with Special Needs',52435,'HQ773','HQ778.7'),('Child Care & Day Care',52435,'HQ778.5','HQ778.7'),
 ('English-Speaking African Government',11884,'JQ1880','JQ3185'),('South African Government',11884,'JQ1900','JQ2699'),('Southern & Central African Government',11884,'JQ2720','JQ2942.2'),('Zambian Government',11884,'JQ2800','JQ2899'),('West African Government',11884,'JQ2998','JQ3121'),('Indian Ocean Island Governments',11884,'JQ3158','JQ3188'),('North African Government',11884,'JQ3198','JQ3349.22'),('Algerian Government',11884,'JQ3200','JQ3299'),('French-Speaking African Government',11884,'JQ3349.5','JQ3494'),('Egyptian Government',11884,'JQ3800','JQ3899');

INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;
