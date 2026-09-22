BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:14251312-253e-5e28-9505-ee4431a5a298';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:banking:general', 'General Banking'),
  ('bokheim:banking:management', 'Bank Management'),
  ('bokheim:banking:credit-interest', 'Bank Credit & Interest'),
  ('bokheim:banking:deposits', 'Deposits & Deposit Insurance'),
  ('bokheim:banking:payments', 'Payments & Banking Transactions'),
  ('bokheim:banking:regulation', 'Bank Regulation & Ownership'),
  ('bokheim:banking:institutions', 'Banks & Financial Institutions'),
  ('bokheim:banking:islamic', 'Islamic Banking & Finance'),
  ('bokheim:banking:by-country', 'Banking by Country');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:banking:general', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 10),
  ('bokheim:banking:management', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 20),
  ('bokheim:banking:credit-interest', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 30),
  ('bokheim:banking:deposits', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 40),
  ('bokheim:banking:payments', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 50),
  ('bokheim:banking:regulation', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 60),
  ('bokheim:banking:institutions', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 70),
  ('bokheim:banking:islamic', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 80),
  ('bokheim:banking:by-country', 'unified:14251312-253e-5e28-9505-ee4431a5a298', 0, 90);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:banking:general', 'bisac', 'BUS004000', 0),
  ('bokheim:banking:general', 'lcc', 'HG1501..HG1614.9@1', 0),
  ('bokheim:banking:management', 'lcc', 'HG1615..HG1616.9@1', 0),
  ('bokheim:banking:credit-interest', 'lcc', 'HG1621..HG1656.9@1', 0),
  ('bokheim:banking:deposits', 'lcc', 'HG1660..HG1662.9@1', 0),
  ('bokheim:banking:payments', 'lcc', 'HG1663..HG1712.9@1', 0),
  ('bokheim:banking:regulation', 'lcc', 'HG1720..HG1778.9@1', 0),
  ('bokheim:banking:institutions', 'lcc', 'HG1779..HG2396.9@1', 0),
  ('bokheim:banking:islamic', 'bisac', 'BUS112000', 0),
  ('bokheim:banking:by-country', 'lcc', 'HG2397..HG3550@1', 0),
  ('bokheim:banking:by-country', 'lcc', 'HG4301..HG4480.9@1', 1);

COMMIT;
