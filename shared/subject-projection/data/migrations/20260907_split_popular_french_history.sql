BEGIN;

-- Repair a partially applied development run whose provisional IDs were
-- concurrently claimed by unrelated concepts.
DELETE FROM lcc_range WHERE class_letters='DC' AND (
  (start_value='420' AND end_value='420.999' AND concept_id=51511) OR
  (start_value='421' AND end_value='421.999' AND concept_id=51512) OR
  (start_value='422' AND end_value='422.999' AND concept_id=51513) OR
  (start_value='423' AND end_value='423.999' AND concept_id=51514) OR
  (start_value='424' AND end_value='424.999' AND concept_id=51515) OR
  (start_value='417' AND end_value='417.999' AND concept_id=51516) OR
  (start_value='715' AND end_value='715.999' AND concept_id=51517)
);

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51530,'Charles de Gaulle, 1958–1969'),
  (51531,'Georges Pompidou, 1969–1974'),
  (51532,'Valéry Giscard d’Estaing, 1974–1981'),
  (51533,'François Mitterrand, 1981–1995'),
  (51534,'Jacques Chirac, 1995–2007'),
  (51535,'Fifth Republic Political & Diplomatic History'),
  (51536,'Paris Social & Cultural History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51530,11465,0),(51530,13353,1),(51531,11465,0),(51531,13353,1),
  (51532,11465,0),(51532,13353,1),(51533,11465,0),(51533,13353,1),
  (51534,11465,0),(51534,13353,1),
  (51535,11465,0),(51535,51031,1),(51536,12745,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DC','420','420.999',51530),('DC','421','421.999',51531),
  ('DC','422','422.999',51532),('DC','423','423.999',51533),
  ('DC','424','424.999',51534),('DC','417','417.999',51535),
  ('DC','715','715.999',51536);

COMMIT;
