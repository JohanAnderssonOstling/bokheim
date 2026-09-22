-- Replace Modern Italy's enumerated DG chronology with one disjoint LC range
-- per existing broad era. No individual event or ruler nodes are introduced.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (8213,9340,9283,6307,9215,8129,8430,9133,8279);

INSERT INTO source_selector VALUES
 (8213,'lcc','DG539..DG541.8.Z'),
 (9340,'lcc','DG542..DG545.8.Z'),
 (9283,'lcc','DG546..DG549'),
 (6307,'lcc','DG550.5..DG554.5.Z'),
 (9215,'lcc','DG555..DG565.Z'),
 (8129,'lcc','DG566..DG570.Z'),
 (8430,'lcc','DG571..DG575.Z'),
 (9133,'lcc','DG576..DG579.Z'),
 (8279,'lcc','DG581..DG583.8.Z');

DELETE FROM source_selector WHERE concept_id=3576 AND system_id='lcc';

COMMIT;
