-- Consolidate official legal-research and science/arts research schedules into
-- the Legal Education & Research leaf. Transfer the European schedule from
-- the generic European Comparative Law subject.
BEGIN;

DELETE FROM source_selector
WHERE concept_id=5712 AND system_id='lcc' AND selector IN (
  'KE3968','KE3990','KE3995','KE4000','KE4010',
  'KEO800.9','KEO801','KEO802',
  'KEQ973','KEQ975',
  'KJC6345','KJC83'
);

DELETE FROM lcc_range
WHERE (class_letters,start_value,end_value) IN (
  VALUES
    ('KE','3968','3986'),
    ('KEO','799.92','800.8'),
    ('KEQ','969.92','971.5'),
    ('KJC','6344','6354'),
    ('KJC','6346','6352'),
    ('KJC','6353','6354'),
    ('KJC','79','82')
);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('KE','3950','4010',5712),
  ('KEO','798','802',5712),
  ('KEQ','968','975',5712),
  ('KJC','6344','6354',5712),
  ('KJC','76','83',5712);

COMMIT;
