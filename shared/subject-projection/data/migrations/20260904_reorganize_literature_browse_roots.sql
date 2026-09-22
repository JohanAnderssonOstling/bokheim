-- Give Literature four concise, purpose-based browse branches while retaining
-- the existing selector-bearing detail for subsequent branch-by-branch pruning.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label = 'By Language'
WHERE concept_id = 'bokheim:subject:literature:language-based';

UPDATE concept SET preferred_label = 'By Region'
WHERE concept_id = 'bokheim:subject:literature:world-regional';

UPDATE concept SET preferred_label = 'Studies'
WHERE concept_id = 'bokheim:subject:literature:theory-criticism';

UPDATE concept_parent SET display_order = 0
WHERE concept_id = 'bokheim:subject:literature:genres'
  AND parent_concept_id = 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854';
UPDATE concept_parent SET display_order = 1
WHERE concept_id = 'bokheim:subject:literature:language-based'
  AND parent_concept_id = 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854';
UPDATE concept_parent SET display_order = 2
WHERE concept_id = 'bokheim:subject:literature:world-regional'
  AND parent_concept_id = 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854';
UPDATE concept_parent SET display_order = 3
WHERE concept_id = 'bokheim:subject:literature:theory-criticism'
  AND parent_concept_id = 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854';

-- Fold the separate reference branch into Studies.
UPDATE concept_parent
SET parent_concept_id = 'bokheim:subject:literature:theory-criticism'
WHERE parent_concept_id = 'bokheim:subject:literature:reference-education';

DELETE FROM concept_parent
WHERE concept_id = 'bokheim:subject:literature:reference-education';
DELETE FROM concept
WHERE concept_id = 'bokheim:subject:literature:reference-education';

-- Short labels are unambiguous inside their new parent path.
UPDATE concept SET preferred_label = 'Criticism'
WHERE concept_id = 'unified:96c1b9c6-90b2-5dbd-ac52-84626372c0cb';
UPDATE concept SET preferred_label = 'History'
WHERE concept_id = 'unified:70b3087b-1a72-5fae-b1e6-97b9b7f739c2';
UPDATE concept SET preferred_label = 'Theory'
WHERE concept_id = 'unified:3a7aa472-aa2e-5910-9fd8-9a3ef391ad07';
UPDATE concept SET preferred_label = 'General'
WHERE concept_id = 'unified:bbcf0a3c-e484-5778-ab84-70596d1c59c9';
UPDATE concept SET preferred_label = 'Rhetoric'
WHERE concept_id = 'unified:c833339a-e53e-557f-aa58-19b907cd90e2';
UPDATE concept SET preferred_label = 'Reference'
WHERE concept_id = 'unified:35b90b6c-ff04-58df-910e-120635cdb872';

-- Shorten the first visible grouping labels without changing their contents.
UPDATE concept SET preferred_label = 'Classical'
WHERE concept_id = 'bokheim:subject:literature:classical';
UPDATE concept SET preferred_label = 'English'
WHERE concept_id = 'bokheim:subject:literature:english';
UPDATE concept SET preferred_label = 'Germanic & Nordic'
WHERE concept_id = 'bokheim:subject:literature:germanic';
UPDATE concept SET preferred_label = 'Romance'
WHERE concept_id = 'bokheim:subject:literature:romance';
UPDATE concept SET preferred_label = 'Slavic & Eurasian'
WHERE concept_id = 'bokheim:subject:literature:slavic-eurasian';

UPDATE concept SET preferred_label = 'Africa & Middle East'
WHERE concept_id = 'bokheim:subject:literature:african-middle-eastern';
UPDATE concept SET preferred_label = 'Asia & Pacific'
WHERE concept_id = 'bokheim:subject:literature:asian-pacific';
UPDATE concept SET preferred_label = 'Indigenous'
WHERE concept_id = 'bokheim:subject:literature:indigenous-other';

COMMIT;
