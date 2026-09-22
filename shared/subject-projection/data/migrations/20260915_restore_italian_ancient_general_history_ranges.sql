-- Restore the general ancient-Italy ranges on the merged Ancient Era node.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
    ('DG203','DG204',3571),
    ('DG205','DG206',3571),
    ('DG207','DG209',3571),
    ('DG214','DG215',3571);

COMMIT;
