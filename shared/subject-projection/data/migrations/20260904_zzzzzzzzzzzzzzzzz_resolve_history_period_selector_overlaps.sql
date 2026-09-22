-- Prefer specific historical periods over their broader country or empire
-- containers for selector ownership.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE history_period_owner(winner TEXT, loser TEXT, PRIMARY KEY(winner,loser)) WITHOUT ROWID;
INSERT INTO history_period_owner VALUES
 ('unified:1770bc31-ed59-5007-b0cb-83c4aa0b1fb9','unified:e535d6ed-0ff5-5b1a-9e73-85371a32b572'),
 ('bokheim:history-lcc-full:baffe5a4-badc-5e9b-a2e0-1c9af5c8a9ee','unified:cad75fff-9e45-5455-bab5-97baa628746d'),
 ('bokheim:history-lcc:d29dc224-5ca3-56ae-a39c-18e9e04abc8a','bokheim:history-lcc:1ff58cac-1728-59f2-9f6a-eeb29a82d5ad'),
 ('bokheim:history-lcc:ae1934aa-738e-545c-94f7-f15f234c5612','bokheim:history-lcc:1ff58cac-1728-59f2-9f6a-eeb29a82d5ad'),
 ('bokheim:history-lcc-full:503690fd-b94b-53af-adf1-cb2acd27a3bc','bokheim:history-lcc:1ff58cac-1728-59f2-9f6a-eeb29a82d5ad'),
 ('bokheim:history-lcc-full:091da0c5-5007-566f-8189-e6343d379825','bokheim:history-lcc:1ff58cac-1728-59f2-9f6a-eeb29a82d5ad'),
 ('bokheim:history-lcc-full:067be3e6-1fb7-5f2e-8134-ec77b6855d78','bokheim:history-lcc:1ff58cac-1728-59f2-9f6a-eeb29a82d5ad'),
 ('bokheim:history-lcc:9f84e5a4-b54d-52ae-8d1b-4c07c1db8816','unified:3d37af1e-3f24-581f-8e49-ca78250004b2');

DELETE FROM source_selector AS losing
WHERE EXISTS (
 SELECT 1 FROM history_period_owner owner
 JOIN source_selector winning
   ON winning.concept_id=owner.winner
  AND winning.system_id=losing.system_id
  AND winning.selector=losing.selector
 WHERE owner.loser=losing.concept_id
);

DROP TABLE history_period_owner;
COMMIT;
