-- Compact terminal subjects whose enumerated selectors form complete,
-- semantically uniform LC schedule blocks.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (9867,13323,13610,13612,4968,13329,4953,13335,8793,8222,10117,13336);

INSERT INTO source_selector VALUES
 (9867,'lcc','T385..T386.Z'),
 (13323,'lcc','Q174..Q175.3'),
 (13610,'lcc','HC59.69..HC59.72.Z'),
 (13612,'lcc','HC51..HC59.3'),
 (4968,'lcc','QA171.48..QA171.5'),
 (4968,'lcc','QA248..QA248.5'),
 (13329,'lcc','QA267..QA268.5'),
 (4953,'lcc','QA188..QA196.7'),
 (13335,'lcc','QA246..QA246.8.Z'),
 (8793,'lcc','DU29..DU68.Z'),
 (8222,'lcc','NB1220..NB1240.Z'),
 (10117,'lcc','DP622..DP629.Z'),
 (13336,'lcc','QA247..QA247.5');

UPDATE concept SET preferred_label='Abstract Machines & Automata'
WHERE concept_id=13329;
UPDATE concept SET preferred_label='Algebraic & Transcendental Numbers'
WHERE concept_id=13336;

COMMIT;
