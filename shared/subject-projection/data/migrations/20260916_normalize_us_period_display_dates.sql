-- Display dates use a single boundary; LCC selectors retain their source scope.
-- Mid-century starts at the end of the Mexican War, as requested.
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label='Early 19th Century, 1809–1845' WHERE concept_id=3411;
UPDATE concept SET preferred_label='Mid-19th Century, 1848–1861' WHERE concept_id=3415;
UPDATE concept SET preferred_label='Revolution to Civil War, 1775–1861' WHERE concept_id=3417;
UPDATE concept SET preferred_label='Mexican War, 1846–1848' WHERE concept_id=3414;
COMMIT;
