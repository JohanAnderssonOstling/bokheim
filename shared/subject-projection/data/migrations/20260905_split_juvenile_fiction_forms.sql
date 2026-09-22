-- Route specific English-language juvenile LCC forms into the existing genre/form tree.
-- General fiction remains on the language subject; BISAC assignments are preserved.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept VALUES('bokheim:juvenile-fiction:story-collections','Story Collections');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:juvenile-fiction:story-collections','bokheim:subject:juvenile-fiction:forms-formats',0,10);
UPDATE concept SET preferred_label='Nursery Rhymes, Rhyming Stories & Songs' WHERE concept_id='unified:65d356cf-7db3-5511-8e84-26c076462d4d';
UPDATE concept SET preferred_label='Fairy Tales, Folklore & Fables' WHERE concept_id='unified:d92ccf72-9b15-57a0-927a-78f804661d5a';
INSERT INTO source_selector VALUES('unified:65d356cf-7db3-5511-8e84-26c076462d4d','lcc','PZ8.3..PZ8.3@5',0);
INSERT INTO source_selector VALUES('unified:d92ccf72-9b15-57a0-927a-78f804661d5a','lcc','PZ8..PZ8@5',0);
INSERT INTO source_selector VALUES('unified:d92ccf72-9b15-57a0-927a-78f804661d5a','lcc','PZ8.1..PZ8.1@5',1);
INSERT INTO source_selector VALUES('unified:d92ccf72-9b15-57a0-927a-78f804661d5a','lcc','PZ8.2..PZ8.2@5',2);
INSERT INTO source_selector VALUES('unified:a8c281f1-aef2-5d28-8022-dce84204157d','lcc','PZ7.7..PZ7.7@5',0);
INSERT INTO source_selector VALUES('unified:6d0d6b55-2bd0-5581-9f4b-3327cab45b9c','lcc','PZ10.3..PZ10.3@5',0);
INSERT INTO source_selector VALUES('bokheim:juvenile-fiction:story-collections','lcc','PZ5..PZ5@5',0);
INSERT INTO source_selector VALUES('unified:41fd2e3c-4a41-52cd-beda-b6e0a3139769','lcc','PZ7.5..PZ7.5@5',0);
COMMIT;
