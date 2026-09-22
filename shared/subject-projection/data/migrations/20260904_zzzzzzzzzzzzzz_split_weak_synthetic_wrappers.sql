-- Dissolve weak synthetic wrappers, promoting their externally classified
-- children to the wrapper's parent. Also remove one empty Catholic wrapper.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH wrapper_destination(wrapper_id, parent_id, display_order) AS (
    SELECT edge.concept_id, edge.parent_concept_id, edge.display_order
    FROM concept_parent edge
    WHERE edge.concept_id IN (
        'bokheim:romance:speculative',
        'bokheim:subject:technology:biomedical-food-pharma',
        'bokheim:education:teacher-development',
        'bokheim:subject:agriculture:economics-education-history',
        'unified:013b9c7d-2354-58cb-aec4-2eb406adfaf0',
        'bokheim:biology:cellular-molecular',
        'bokheim:education:leadership-institutions',
        'bokheim:biology:physiology-reproduction'
    )
)
UPDATE concept_parent
SET parent_concept_id = (
        SELECT destination.parent_id
        FROM wrapper_destination destination
        WHERE destination.wrapper_id = concept_parent.parent_concept_id
    ),
    display_order = (
        SELECT destination.display_order
        FROM wrapper_destination destination
        WHERE destination.wrapper_id = concept_parent.parent_concept_id
    )
WHERE parent_concept_id IN (SELECT wrapper_id FROM wrapper_destination);

DELETE FROM concept
WHERE concept_id IN (
    'bokheim:romance:speculative',
    'bokheim:subject:technology:biomedical-food-pharma',
    'bokheim:education:teacher-development',
    'bokheim:subject:agriculture:economics-education-history',
    'unified:013b9c7d-2354-58cb-aec4-2eb406adfaf0',
    'bokheim:biology:cellular-molecular',
    'bokheim:education:leadership-institutions',
    'bokheim:biology:physiology-reproduction',
    'bokheim:catholic:doctrine-preaching-relations'
);

COMMIT;
