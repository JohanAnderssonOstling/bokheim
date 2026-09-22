-- Resolve the concrete discrepancies in the History period/code audit.
-- Reference: bundled master LCC headings; E840.6–E840.8 additionally verified
-- against https://www.loc.gov/aba/publications/Archived-LCC2022/LCC_E-F2022OUT.pdf.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_label_fix(concept_id INTEGER PRIMARY KEY, label TEXT NOT NULL);
INSERT INTO period_label_fix VALUES
  (10138,'1814–1907'),
  (10391,'1918–1939'),
  (10428,'1968–1992'),
  (10460,'1989–1993'),
  (10199,'Union & Republic, 1918–1993'),
  (10361,'Post-Cold War Europe, 1989–Present'),
  (10604,'Postwar Europe, 1945–Present'),
  (51992,'1945–Present'),
  (51005,'1961–2000'),
  (51006,'1961–2000'),
  (51007,'1961–2000');

INSERT INTO source_label(concept_id,label,ordinal)
SELECT c.concept_id,c.preferred_label,
       coalesce((SELECT max(s.ordinal)+1 FROM source_label s
                 WHERE s.concept_id=c.concept_id),0)
FROM concept c JOIN period_label_fix f USING(concept_id)
WHERE c.preferred_label<>f.label
AND NOT EXISTS(SELECT 1 FROM source_label s WHERE s.concept_id=c.concept_id
               AND s.label=c.preferred_label);
UPDATE concept SET preferred_label=f.label
FROM period_label_fix f WHERE concept.concept_id=f.concept_id;

-- Exact master ranges restore the two empty period/event subjects.
-- The U.S. topical ranges cover the later twentieth century, not just the
-- Kennedy/Johnson administrations. More specific existing selectors remain.
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('DG532','DG537.8',3569),
  ('E351','E364.9',3693),
  ('E839.5','E839.8',51005),
  ('E840.4','E840.4',51006),
  ('E840.6','E840.8',51007)
ON CONFLICT(start_code,end_code) DO NOTHING;

DROP TABLE period_label_fix;
COMMIT;
