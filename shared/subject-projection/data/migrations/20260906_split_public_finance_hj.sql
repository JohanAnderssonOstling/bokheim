BEGIN IMMEDIATE;

-- Merge the LCC-only duplicate into the existing BISAC Public Finance concept.
DELETE FROM source_selector
WHERE concept_id = 'unified:30404046-aacd-5389-9194-9cea81860de8';
DELETE FROM concept_parent
WHERE concept_id = 'unified:30404046-aacd-5389-9194-9cea81860de8';
DELETE FROM source_label
WHERE concept_id = 'unified:30404046-aacd-5389-9194-9cea81860de8';
DELETE FROM concept
WHERE concept_id = 'unified:30404046-aacd-5389-9194-9cea81860de8';

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order)
VALUES ('unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9',
        'bokheim:sociology:change-development', 1, 60);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:public-finance:reference-history', 'Public Finance — Reference & History'),
  ('bokheim:public-finance:regional', 'Public Finance by Region or Country'),
  ('bokheim:public-finance:general', 'Other Public Finance'),
  ('bokheim:public-finance:customs', 'Customs Administration'),
  ('bokheim:public-finance:spending', 'Government Spending'),
  ('bokheim:public-finance:debt', 'Public Debt'),
  ('bokheim:public-finance:local', 'Local & Municipal Finance'),
  ('bokheim:public-finance:accounting', 'Public Accounting & Auditing');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:public-finance:reference-history', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 10),
  ('bokheim:public-finance:regional', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 20),
  ('bokheim:public-finance:general', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 30),
  ('unified:809d0118-bd83-5877-8673-70efe4c2737d', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 1, 40),
  ('unified:202599a1-946c-5cfc-9980-6a72cd04eba4', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 1, 50),
  ('bokheim:public-finance:customs', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 60),
  ('bokheim:public-finance:spending', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 70),
  ('bokheim:public-finance:debt', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 80),
  ('bokheim:public-finance:local', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 90),
  ('bokheim:public-finance:accounting', 'unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 0, 100);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:public-finance:reference-history', 'lcc', 'HJ9..HJ240@1', 0),
  ('bokheim:public-finance:regional', 'lcc', 'HJ241..HJ1620@1', 0),
  ('bokheim:public-finance:general', 'lcc', 'HJ1620.1..HJ2004.9@1', 0),
  ('unified:809d0118-bd83-5877-8673-70efe4c2737d', 'lcc', 'HJ2005..HJ2216@1', 0),
  ('unified:202599a1-946c-5cfc-9980-6a72cd04eba4', 'lcc', 'HJ2240..HJ5908@1', 0),
  ('bokheim:public-finance:general', 'lcc', 'HJ5908.1..HJ6602.9@1', 1),
  ('bokheim:public-finance:customs', 'lcc', 'HJ6603..HJ7390@1', 0),
  ('bokheim:public-finance:general', 'lcc', 'HJ7390.1..HJ7460.9@1', 2),
  ('bokheim:public-finance:spending', 'lcc', 'HJ7461..HJ7980@1', 0),
  ('bokheim:public-finance:general', 'lcc', 'HJ7980.1..HJ8000.9@1', 3),
  ('bokheim:public-finance:debt', 'lcc', 'HJ8001..HJ8899@1', 0),
  ('bokheim:public-finance:general', 'lcc', 'HJ8899.1..HJ9102.9@1', 4),
  ('bokheim:public-finance:local', 'lcc', 'HJ9103..HJ9695@1', 0),
  ('bokheim:public-finance:general', 'lcc', 'HJ9695.1..HJ9700.9@1', 5),
  ('bokheim:public-finance:accounting', 'lcc', 'HJ9701..HJ9995@1', 0);

COMMIT;
