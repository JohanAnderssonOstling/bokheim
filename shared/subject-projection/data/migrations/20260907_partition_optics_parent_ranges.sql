-- All specialist optics divisions begin at QC367. The occupied opening span is
-- reference, biography, textbooks, and study; exact history headings override it.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53346,'Optics Reference, Biography & Study');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53346,5040,0);

DELETE FROM lcc_range WHERE concept_id=5040;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QC350','QC366',53346);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5040,'QC350-467');

DELETE FROM lcc_range
WHERE concept_id=9513
  AND start_code='QC374'
  AND end_code='QC379';
