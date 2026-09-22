-- Compact unambiguous thematic and modern German history spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _de_base(id INTEGER NOT NULL);
INSERT INTO _de_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _de_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _de_node VALUES
 (1,'German Antiquities','DD51..DD55.2'),
 (2,'German Social & Cultural History','DD60..DD61'),
 (3,'German Ethnography','DD73..DD78'),
 (4,'Reunified Germany, 1990–','DD290..DD290.33.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _de_node n CROSS JOIN _de_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3524,0 FROM _de_node n CROSS JOIN _de_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _de_node n CROSS JOIN _de_base b;

DELETE FROM source_selector WHERE concept_id=3524 AND system_id='lcc' AND (
 selector GLOB 'DD51*' OR selector GLOB 'DD52*' OR selector GLOB 'DD53*'
 OR selector GLOB 'DD54*' OR selector GLOB 'DD55*' OR selector GLOB 'DD60*'
 OR selector GLOB 'DD61*' OR selector GLOB 'DD73*'
 OR selector GLOB 'DD74*' OR selector GLOB 'DD75*' OR selector GLOB 'DD76*'
 OR selector GLOB 'DD77*' OR selector GLOB 'DD78*' OR selector GLOB 'DD290*'
 OR selector='DD851..DD900'
);

DROP TABLE _de_node;
DROP TABLE _de_base;
COMMIT;
