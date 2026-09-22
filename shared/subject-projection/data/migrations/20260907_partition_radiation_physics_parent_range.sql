-- The official specialist sequence begins at QC476.4. Keep the occupied
-- general opening on a leaf and preserve the printed whole division exactly.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53347,'Radiation Physics Reference & General Theory');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53347,5041,0);

DELETE FROM lcc_range WHERE concept_id=5041;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QC474','QC476.3',53347);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5041,'QC474-496.9');
