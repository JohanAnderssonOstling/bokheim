BEGIN;

-- These are all nonleaf concepts. Their former class-wide ranges overlapped
-- their curated children and made one LCC call number resolve broadly as well
-- as specifically. Retain exact umbrella codes, but permissive coverage belongs
-- exclusively to explicit leaves.
DELETE FROM lcc_range WHERE concept_id IN (
 4558, -- Christian Denominations: BX1–BX9999
 4549, -- Religion: BL1–BL2790
 4582, -- Christianity: BR1–BR1725
 4551, -- Bible: BS1–BS2970
 4544, -- Metaphysics & Ontology: BD10–BD701
 4615, -- Judaism: BM1–BM990
 3798, -- Oriental Philology & Literature: PJ1–PJ9500
 3749, -- Germanic Languages: PD1–PD7159
 7534, -- Applied Ethics: BJ1545–BJ1691
 4553, -- Buddhism: BQ1–BQ9800
 4536  -- Ontology: BD300–BD450
);

COMMIT;
