-- Remove the 13 explicitly approved zero-hit subjects and their selectors.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
-- Technique of blank verse (PN1055.62)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:30ebd5b0-3227-55c7-8972-03c66062877c';
-- Central America (Z887)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b677fc23-fc00-5cb3-b345-1546d9a73536';
-- Baking powders, etc. (TX409)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e3f7e1db-f2d1-5477-965f-74a066e96380';
-- Dialects. Provincialisms, etc. (PK1545)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4592e4f3-b24b-553c-9683-ae20def4c849';
-- Preliminary operations (TF208)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0660a80e-877b-51d2-8158-3cbe6c539960';
-- Gutta-percha (TS1930)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5939014e-234f-57a5-8d5f-457b8b200b57';
-- Electric radiations (General) (QC483)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a79eb509-7f27-5233-84f9-d83e9698f3f5';
-- Kolāmī (PL4681)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:02cd0ace-b745-5f1d-b6b2-8613532646e5';
-- Therapeutics (RF124)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2dbab39a-ea3c-574b-bbe9-694b9a707252';
-- Hand cameras (TR260)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1630dc03-8792-5532-97c2-f03b18c475e8';
-- Teratology (QK664)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:db48817e-e49a-5279-96bf-4c2c279ecccc';
-- Immune sera. Convalescent sera (RM279)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:275fd9bc-fe31-5166-96b6-3f6ece47f7a8';
-- Turkmenistan (B4776..B4780@7)
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:04e0b450-36f8-5d4a-9e3e-5ce86c73bc55';
COMMIT;
