-- Further official leaf compaction. Combinatorics deliberately excludes the
-- separately curated QA166..QA166.247 Graph Theory range.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=9547 AND system_id='lcc';
INSERT INTO source_selector VALUES(9547,'lcc','NC977..NC989.Z');

DELETE FROM source_selector WHERE concept_id=11663 AND system_id='lcc';
INSERT INTO source_selector VALUES(11663,'lcc','QA76.55..QA76.57.Z');

DELETE FROM source_selector WHERE concept_id=4927 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (4927,'lcc','QA164..QA165'),
 (4927,'lcc','QA166.25..QA167.2');

DELETE FROM source_selector WHERE concept_id=4921 AND system_id='lcc';
INSERT INTO source_selector VALUES(4921,'lcc','QA184..QA205');

DELETE FROM source_selector WHERE concept_id=3927 AND system_id='lcc';
INSERT INTO source_selector VALUES(3927,'lcc','PR750..PR890.Z');

DELETE FROM source_selector WHERE concept_id=3250 AND system_id='lcc';
INSERT INTO source_selector VALUES(3250,'lcc','E176.1..E176.49');

DELETE FROM source_selector WHERE concept_id=10405 AND system_id='lcc';
INSERT INTO source_selector VALUES(10405,'lcc','F1030..F1030.9');

DELETE FROM source_selector WHERE concept_id=12725 AND system_id='lcc';
INSERT INTO source_selector VALUES(12725,'lcc','HV6080..HV6113.Z');

DELETE FROM source_selector WHERE concept_id=4918 AND system_id='lcc'
 AND selector GLOB 'QA25*';
INSERT INTO source_selector VALUES(4918,'lcc','QA251..QA252.5');

COMMIT;
