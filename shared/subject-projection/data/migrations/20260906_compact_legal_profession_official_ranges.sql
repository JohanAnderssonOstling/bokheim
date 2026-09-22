-- Consolidate legal-profession schedules by jurisdiction. The European
-- profession span is removed from the generic comparative-law leaf so each
-- selector remains owned by the most specific subject.
BEGIN;

DELETE FROM source_selector
WHERE concept_id = 5711
  AND system_id = 'lcc'
  AND (
    selector GLOB 'KBM16*' OR
    selector GLOB 'KBP16*' OR
    selector GLOB 'KBR382*' OR
    selector GLOB 'KJC37*' OR
    selector GLOB 'KK37*' OR
    selector GLOB 'KKA37*'
  );

DELETE FROM source_selector
WHERE concept_id = 5317
  AND system_id = 'lcc'
  AND selector = 'KJC3704..KJC3711';

INSERT INTO source_selector(concept_id, system_id, selector) VALUES
  (5711, 'lcc', 'KBM1610..KBM1630'),
  (5711, 'lcc', 'KBP1610..KBP1637'),
  (5711, 'lcc', 'KBR3822.5..KBR3824'),
  (5711, 'lcc', 'KJC3704..KJC3791'),
  (5711, 'lcc', 'KK3704..KK3792.2'),
  (5711, 'lcc', 'KKA3704..KKA3771');

COMMIT;
