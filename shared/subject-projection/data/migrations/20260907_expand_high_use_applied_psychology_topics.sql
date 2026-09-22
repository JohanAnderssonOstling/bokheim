BEGIN;

-- Named BF637 Cutter topics with at least 40 Open Library matches.  These are
-- exact selectors: the alphabetical gaps and sparse topics remain covered by
-- Applied Psychology's BF636-BF637.2 fallback range.
CREATE TEMP TABLE _applied_psychology_topic(
    label TEXT NOT NULL,
    selector TEXT NOT NULL,
    existing_concept_id INTEGER
);

INSERT INTO _applied_psychology_topic VALUES
 ('Self-Actualization',                         'BF637.S4',  NULL),
 ('Success',                                    'BF637.S8',  3194),
 ('Interpersonal Communication',                'BF637.C45', NULL),
 ('Group Counseling',                           'BF637.C6',  NULL),
 ('Meditation',                                 'BF637.M4',  NULL),
 ('Consultation',                               'BF637.C56', NULL),
 ('Personal Coaching',                          'BF637.P36', NULL),
 ('Mindfulness',                                'BF637.M56', NULL),
 ('Negotiation',                                'BF637.N4',  NULL),
 ('Neuro-Linguistic Programming',               'BF637.N46', 3189),
 ('Body Language & Nonverbal Communication',    'BF637.N66', NULL),
 ('Behavior Modification',                      'BF637.B4',  NULL),
 ('Conduct of Life',                            'BF637.C5',  NULL);

INSERT INTO concept(preferred_label)
SELECT label
FROM _applied_psychology_topic
WHERE existing_concept_id IS NULL;

CREATE TEMP TABLE _applied_psychology_resolved AS
SELECT t.label,
       t.selector,
       COALESCE(t.existing_concept_id, c.concept_id) AS concept_id
FROM _applied_psychology_topic t
LEFT JOIN concept c
  ON t.existing_concept_id IS NULL
 AND c.preferred_label=t.label;

-- New concepts use ordinal zero. Reused concepts already use ordinal zero for
-- their original parent, so their second parent relationship uses ordinal one.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5384,
       CASE WHEN concept_id IN (3189,3194) THEN 1 ELSE 0 END
FROM _applied_psychology_resolved
WHERE NOT EXISTS (
    SELECT 1 FROM concept_parent cp
    WHERE cp.concept_id=_applied_psychology_resolved.concept_id
      AND cp.parent_concept_id=5384
);

INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,selector
FROM _applied_psychology_resolved;

DROP TABLE _applied_psychology_resolved;
DROP TABLE _applied_psychology_topic;
COMMIT;
