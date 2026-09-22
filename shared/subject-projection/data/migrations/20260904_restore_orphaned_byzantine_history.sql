-- Restore retained Byzantine-history concepts whose parent edges were removed
-- while pruning unrelated regional and chronological intermediates.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
    ('bokheim:history-lcc:f095ba84-bcf7-5a99-9fa9-6931d0dfffff',
     'unified:f94f9394-6a40-5667-b600-a33cb82644c2',0,0),
    ('bokheim:history-lcc-full:1ed4ec17-b843-5829-bb8a-2ddf9e9bc761',
     'unified:f94f9394-6a40-5667-b600-a33cb82644c2',0,0),
    ('bokheim:lcc-detail:f48ff93f-59d1-5d61-9445-86eb4e7331b4',
     'unified:f94f9394-6a40-5667-b600-a33cb82644c2',0,0),
    ('bokheim:lcc-detail:de25acb1-9ff5-574f-8112-a1da726338f3',
     'unified:f94f9394-6a40-5667-b600-a33cb82644c2',0,0),
    ('bokheim:lcc-detail:4ff562ce-1ca4-5ba4-98a3-56c51556ecad',
     'unified:f94f9394-6a40-5667-b600-a33cb82644c2',0,0),
    ('bokheim:history-lcc-full:ec8a67ee-9b7f-581e-8ab3-fbf681ce9bd3',
     'bokheim:history-lcc:f095ba84-bcf7-5a99-9fa9-6931d0dfffff',0,0),
    ('bokheim:history-lcc-full:9f47c50d-d757-5d6b-ae08-381a3973cef7',
     'bokheim:history-lcc:f095ba84-bcf7-5a99-9fa9-6931d0dfffff',0,0),
    ('bokheim:history-lcc-full:eb3cb688-cf54-5a0f-8488-b310f37ce1bb',
     'bokheim:history-lcc-full:9f47c50d-d757-5d6b-ae08-381a3973cef7',0,0),
    ('bokheim:history-lcc-full:346dd14d-81d1-5bbb-bc53-b9c4dd63d78d',
     'bokheim:history-lcc:141a8ae9-e6cf-5bc8-8dae-cc2fefa33e7d',0,0);

COMMIT;
