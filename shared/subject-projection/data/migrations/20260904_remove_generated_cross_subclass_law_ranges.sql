-- Remove table-expansion artifacts that were encoded as continuous LCC
-- ranges across many independent jurisdiction subclasses.  The endpoints in
-- these selectors are corresponding regional-law schedules, not inclusive
-- lexical ranges; treating them as ranges makes unrelated codes between the
-- endpoints match the same topic.
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id = 'lcc'
  AND (
       selector GLOB 'KDZ*..KH*@*'
    OR selector GLOB 'KJ*..KKZ*@*'
    OR selector GLOB 'KL*..KWX*@*'
    OR selector GLOB 'KFA*..KFW*@*'
  );

COMMIT;
