-- C: general auxiliary historical studies, beneath the existing History branch.
-- DDC 935: Mesopotamia & Iranian Plateau to 637, not ancient philosophy.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE history_additions(label TEXT,parent INTEGER,id INTEGER);
INSERT INTO history_additions(label,parent) VALUES
 ('General',3514),
 ('Mesopotamia & Iranian Plateau, to 637',3480);
UPDATE history_additions SET id=(SELECT c.concept_id FROM concept c
 JOIN concept_parent cp USING(concept_id)
 WHERE cp.parent_concept_id=history_additions.parent AND c.preferred_label=history_additions.label);
INSERT INTO concept(preferred_label) SELECT label FROM history_additions WHERE id IS NULL;
UPDATE history_additions SET id=(SELECT max(concept_id) FROM concept WHERE preferred_label=history_additions.label)
WHERE id IS NULL;
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id,parent,0 FROM history_additions;
CREATE TEMP TABLE assignment_guard(ok INTEGER CHECK(ok=1));
INSERT INTO assignment_guard SELECT NOT EXISTS
 (SELECT 1 FROM lcc_selector WHERE selector='C'
  AND concept_id<>(SELECT id FROM history_additions WHERE parent=3514));
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT id,'C' FROM history_additions WHERE parent=3514;
UPDATE ddc_selector SET concept_id=(SELECT id FROM history_additions WHERE parent=3480)
WHERE selector='935';
DROP TABLE assignment_guard;
DROP TABLE history_additions;
COMMIT;
