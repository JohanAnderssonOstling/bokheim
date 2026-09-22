-- Economic History is a navigational node.  HC1-HC1085.2 overlaps all of its
-- chronological, topical and regional children, so it must not be a
-- permissive parent range.  Preserve bare/heading exact selectors on the
-- parent and express the three remaining official regional spans as ranges on
-- their existing leaf subjects.

DELETE FROM lcc_range
WHERE concept_id=5091
  AND start_code='HC1'
  AND end_code='HC1085.2';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('HC240','HC407',11988),
  ('HC411','HC495',11989),
  ('HC800','HC1085',11990);

DELETE FROM lcc_selector
WHERE (concept_id,selector) IN (
  (11988,'HC240-407'),
  (11989,'HC411-495'),
  (11990,'HC800-1085')
);
