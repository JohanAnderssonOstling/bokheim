-- LCC classifies geographic guidebooks inside DG (History of Italy).  Exact
-- selectors take precedence over the broad DG history ranges and place these
-- works in the reader-facing Travel taxonomy instead.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG62',1),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG416',2),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG612',3),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG632',4),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG652',5),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG659.52',6),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG672',7),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG732',8),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG792',9),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG804',10),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG842',11),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG862',12),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG975.B58',13),
 ('unified:754dd3b1-3396-5455-ad8a-d97cd1e660d0','lcc','DG975.B79',14);

COMMIT;
