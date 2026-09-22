-- Add the broad national or language literature ranges from the LCC outline
-- to their parent concepts. Specific child selectors remain below them.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector (concept_id, selector) VALUES
  (4244, 'PT1-PT4897'),
  (3899, 'PT5001-PT5980'),
  (4235, 'PT6000-PT6467.36'),
  (3847, 'PT6500-PT6593.36'),
  (4402, 'PT7101-PT7338'),
  (4389, 'PT7351-PT7550'),
  (3931, 'PT7581-PT7599'),
  (3869, 'PT7601-PT8260'),
  (4394, 'PT8301-PT9155'),
  (5585, 'PR1-PR9680'),
  (5727, 'PS1-PS3626'),
  (4161, 'PS8001-PS8599'),
  (4184, 'PQ1-PQ3999'),
  (4202, 'PQ4001-PQ5999'),
  (4226, 'PQ6001-PQ8929'),
  (4215, 'PQ9000-PQ9999');

COMMIT;
