-- Give every remaining external selector one most-specific curated owner.
-- Broader concepts continue to match through taxonomy ancestry.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE selector_owner(
    winner TEXT NOT NULL,
    loser TEXT NOT NULL,
    PRIMARY KEY(winner, loser)
) WITHOUT ROWID;

INSERT INTO selector_owner VALUES
    -- Christianity: specific doctrines and denominations own their schedules.
    ('unified:3c31a4c9-f232-50b1-b2f8-c69b1bdf83f3','bokheim:christian-theology:christ-salvation'),
    ('unified:2ecc5011-f950-50dc-9983-de9370ca7b79','bokheim:christian-theology:christ-salvation'),
    ('unified:da2b82e3-992d-5056-b5ed-99a183801264','bokheim:christian-theology:christ-salvation'),
    ('unified:74db0a97-60c6-56bd-bc7c-27498015d201','bokheim:christian-theology:doctrine-systematic'),
    ('unified:88dc9a38-69a3-5019-9086-820b28c6dd96','unified:79f83052-ab3b-5950-94d8-de07af5d1258'),
    ('unified:bf1172da-5b27-5cca-97df-13c2de7071ed','unified:79f83052-ab3b-5950-94d8-de07af5d1258'),
    ('unified:bb612d78-008d-546f-abf0-d9a2757dbe3d','unified:79f83052-ab3b-5950-94d8-de07af5d1258'),
    ('unified:5fb2d52c-b4b8-5719-ba39-9e40dec359ce','unified:79f83052-ab3b-5950-94d8-de07af5d1258'),
    ('unified:25c4a755-9c54-5eb9-8e03-c14b77f4767d','unified:79f83052-ab3b-5950-94d8-de07af5d1258'),

    -- Hydrology: the five named specialties own their narrower ranges.
    ('bokheim:lcc-detail:6d5a47f2-e3a6-5212-acd1-ead0bf4ea440','unified:e5b922ca-295d-5925-a57b-0fe0aa15837a'),
    ('bokheim:lcc-detail:49a782a5-7ac5-5e30-aa7d-699d5adcf608','unified:e5b922ca-295d-5925-a57b-0fe0aa15837a'),
    ('bokheim:lcc-detail:2081652f-211b-5ae7-8d51-aede0821a3be','unified:e5b922ca-295d-5925-a57b-0fe0aa15837a'),
    ('bokheim:lcc-detail:c5cb2e05-097a-51e6-8963-0c48276ba055','unified:e5b922ca-295d-5925-a57b-0fe0aa15837a'),
    ('bokheim:lcc-detail:9db47e1f-99db-55e1-ab36-7840a2028372','unified:e5b922ca-295d-5925-a57b-0fe0aa15837a'),

    -- History: retain the most specific facet or named conflict.
    ('bokheim:lcc-detail:de25acb1-9ff5-574f-8112-a1da726338f3','unified:f94f9394-6a40-5667-b600-a33cb82644c2'),
    ('unified:431a37be-2891-50f1-b358-b19887abe619','unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
    ('unified:431a37be-2891-50f1-b358-b19887abe619','unified:8466c640-4128-555f-94a4-b6244eec5bdc'),
    ('unified:44463786-4323-5f18-9a1a-c89e2d8be8e8','unified:3b6825a7-b0fc-5d4c-94d2-9b18413d3cd5'),

    -- The source schedules explicitly name Mobile robots and Teaching aids.
    ('bokheim:lcc-detail:21dc3951-6259-59ee-8db3-338556db706d','bokheim:lcc-detail:dabee6ab-2b16-5dc2-85a9-228fa4927f18'),
    ('bokheim:lcc-detail:4f08b691-1d04-5e15-b59a-41e5b4de668f','bokheim:lcc-detail:9bb6c090-4bf6-5ebe-a686-35acabaeabfb'),

    -- BISAC SCI003000 is explicitly Science / Applied Sciences.
    ('unified:22ed848f-f758-5528-a501-4cca0722a7cf','unified:2d01b451-4e64-5f6b-a068-a6f7ea63e386');

DELETE FROM source_selector AS losing
WHERE EXISTS (
    SELECT 1
    FROM selector_owner ownership
    JOIN source_selector winning
      ON winning.concept_id=ownership.winner
     AND winning.system_id=losing.system_id
     AND winning.selector=losing.selector
    WHERE ownership.loser=losing.concept_id
);

DROP TABLE selector_owner;
COMMIT;
