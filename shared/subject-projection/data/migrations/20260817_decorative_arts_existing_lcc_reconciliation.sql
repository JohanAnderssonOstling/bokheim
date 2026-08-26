-- Reconcile detailed LCC Other arts and art industries captions with
-- equivalent concepts that already exist in the unified taxonomy.
-- No concepts are created by this migration.
-- Source: Library of Congress MDSConnect Classification 2016 retrospective.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Give the existing concepts an additional visible route beneath the current
-- LCC leaf. Ordinal 1 preserves each concept's original primary route.
INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:e45229f3-ed43-505f-86c3-3689cd4466bd', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 10),
    ('unified:e5243a6c-3eb3-5bdc-b213-838bbb0323f4', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 20),
    ('unified:10788ac3-f029-5ac1-80c5-189404332ef4', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 30),
    ('bokheim:crafts:paper-book', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 40),
    ('unified:3284d05f-7a4a-50fd-b701-647f9b4e403a', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 50),
    ('unified:eab565ea-8015-5f1f-813f-af27c5b97e40', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 60),
    ('unified:e119feac-cad4-52f3-aee3-d1735ddc3f7f', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 70),
    ('bokheim:antiques:toys-dolls', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 80),
    ('unified:d36c1a2c-13a7-5b66-9f28-6ea85281667a', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 90),
    ('unified:ba96cf62-1153-5ea0-8b2d-3baf9bd22868', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 100),
    ('unified:34f89cea-cbbc-5bfe-b857-2e8c764a7bf5', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 110),
    ('unified:c6634238-580a-5cb0-9bdd-5ed6ffa80379', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 120),
    ('unified:2f22c210-698a-5ee8-9a5f-027b6561799d', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 130),
    ('unified:40fc8881-ef08-5ee0-ab9a-189d69ca7a52', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 140),
    ('unified:2d38a61c-7070-5fb5-bec1-faa6b5d8b28d', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 150),
    ('unified:cbecdcfc-5ed2-5a35-aa3c-5b81888201b9', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 160),
    ('unified:28e49aa3-ef57-5307-b6ea-5bd0de053514', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 170),
    ('unified:7651ca41-3101-5f2c-9485-df85125fa672', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 180),
    ('unified:4cb419f7-b821-5867-a4c9-79983c1b59e8', 'unified:ea9d2b17-8c87-53ca-bfbb-57e83b6deaa7', 1, 190);

-- Priority 4 makes these mappings more specific than the current priority-3
-- ranges retained on Other arts and art industries.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:e45229f3-ed43-505f-86c3-3689cd4466bd', 'lcc', 'NK3600..NK3640@4', 0),
    ('unified:e5243a6c-3eb3-5bdc-b213-838bbb0323f4', 'lcc', 'NK3649.5..NK3649.55@4', 0),
    ('unified:e5243a6c-3eb3-5bdc-b213-838bbb0323f4', 'lcc', 'NK9587..NK9587@4', 1),
    ('unified:10788ac3-f029-5ac1-80c5-189404332ef4', 'lcc', 'NK3650..NK3650.5@4', 0),
    ('bokheim:crafts:paper-book', 'lcc', 'NK3660..NK3660@4', 0),
    ('unified:3284d05f-7a4a-50fd-b701-647f9b4e403a', 'lcc', 'NK3668.5..NK3670@4', 0),
    ('unified:eab565ea-8015-5f1f-813f-af27c5b97e40', 'lcc', 'NK3700..NK4695@4', 0),
    ('unified:e119feac-cad4-52f3-aee3-d1735ddc3f7f', 'lcc', 'NK4700..NK4890@4', 0),
    ('bokheim:antiques:toys-dolls', 'lcc', 'NK4891.3..NK4894.4@4', 0),
    ('unified:d36c1a2c-13a7-5b66-9f28-6ea85281667a', 'lcc', 'NK5100..NK5440@4', 0),
    ('unified:ba96cf62-1153-5ea0-8b2d-3baf9bd22868', 'lcc', 'NK6200..NK6210@4', 0),
    ('unified:34f89cea-cbbc-5bfe-b857-2e8c764a7bf5', 'lcc', 'NK6400..NK8459@4', 0),
    ('unified:c6634238-580a-5cb0-9bdd-5ed6ffa80379', 'lcc', 'NK8470..NK8475@4', 0),
    ('unified:2f22c210-698a-5ee8-9a5f-027b6561799d', 'lcc', 'NK8552.5..NK8554@4', 0),
    ('unified:2f22c210-698a-5ee8-9a5f-027b6561799d', 'lcc', 'NK8555..NK8555.5@4', 1),
    ('unified:2f22c210-698a-5ee8-9a5f-027b6561799d', 'lcc', 'NK9510..NK9510@4', 2),
    ('unified:40fc8881-ef08-5ee0-ab9a-189d69ca7a52', 'lcc', 'NK8560..NK8560@4', 0),
    ('unified:2d38a61c-7070-5fb5-bec1-faa6b5d8b28d', 'lcc', 'NK8650..NK8669@4', 0),
    ('unified:cbecdcfc-5ed2-5a35-aa3c-5b81888201b9', 'lcc', 'NK8740..NK8740.5@4', 0),
    ('unified:28e49aa3-ef57-5307-b6ea-5bd0de053514', 'lcc', 'NK8800..NK9505.5@4', 0),
    ('unified:7651ca41-3101-5f2c-9485-df85125fa672', 'lcc', 'NK9509..NK9509.95@4', 0),
    ('unified:4cb419f7-b821-5867-a4c9-79983c1b59e8', 'lcc', 'NK9600..NK9699@4', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:e45229f3-ed43-505f-86c3-3689cd4466bd', 'Alphabets. Calligraphy. Initials', 0),
    ('unified:e5243a6c-3eb3-5bdc-b213-838bbb0323f4', 'Basketwork', 0),
    ('unified:e5243a6c-3eb3-5bdc-b213-838bbb0323f4', 'Wickerwork', 1),
    ('unified:10788ac3-f029-5ac1-80c5-189404332ef4', 'Beadwork. Ojime', 0),
    ('bokheim:crafts:paper-book', 'Book ornamentation', 0),
    ('unified:3284d05f-7a4a-50fd-b701-647f9b4e403a', 'Buttons. Insignia', 0),
    ('unified:eab565ea-8015-5f1f-813f-af27c5b97e40', 'Ceramics', 0),
    ('unified:e119feac-cad4-52f3-aee3-d1735ddc3f7f', 'Costume and its accessories', 0),
    ('bokheim:antiques:toys-dolls', 'Dolls and dollhouses', 0),
    ('unified:d36c1a2c-13a7-5b66-9f28-6ea85281667a', 'Glass', 0),
    ('unified:ba96cf62-1153-5ea0-8b2d-3baf9bd22868', 'Leatherwork', 0),
    ('unified:34f89cea-cbbc-5bfe-b857-2e8c764a7bf5', 'Metalwork', 0),
    ('unified:c6634238-580a-5cb0-9bdd-5ed6ffa80379', 'Miniature objects', 0),
    ('unified:2f22c210-698a-5ee8-9a5f-027b6561799d', 'Paper and paper articles. Decorated paper. Decoupage', 0),
    ('unified:2f22c210-698a-5ee8-9a5f-027b6561799d', 'Papier mâché', 1),
    ('unified:2f22c210-698a-5ee8-9a5f-027b6561799d', 'Transfer pictures (Decalcomania)', 2),
    ('unified:40fc8881-ef08-5ee0-ab9a-189d69ca7a52', 'Pattern printing (Handicraft)', 0),
    ('unified:2d38a61c-7070-5fb5-bec1-faa6b5d8b28d', 'Stencil work (Decorative)', 0),
    ('unified:cbecdcfc-5ed2-5a35-aa3c-5b81888201b9', 'Teddy bears', 0),
    ('unified:28e49aa3-ef57-5307-b6ea-5bd0de053514', 'Textile arts and art needlework', 0),
    ('unified:7651ca41-3101-5f2c-9485-df85125fa672', 'Toys', 0),
    ('unified:4cb419f7-b821-5867-a4c9-79983c1b59e8', 'Woodwork', 0);

COMMIT;
