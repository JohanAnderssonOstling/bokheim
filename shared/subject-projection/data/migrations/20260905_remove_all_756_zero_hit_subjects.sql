-- Remove all 756 approved subjects with no recorded hits in their entire subtree.
-- Includes all attached selectors; retains the 32 zero-direct-hit subjects with descendant hits.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
-- Synthetic Biology
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:synthetic-biology';
-- 3D Food Printing
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:3d-food-printing';
-- Sentiment Analysis
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:sentiment-analysis';
-- Support Vector Machines
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:support-vector-machines';
-- Transfer Learning
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:transfer-learning';
-- Bash
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:bash';
-- GraphQL
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:graphql';
-- Hopscotch
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:hopscotch';
-- Pseudocode
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:pseudocode';
-- Telepresence
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:telepresence';
-- Tor
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:tor';
-- Svelte
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:svelte';
-- Diameter
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:diameter';
-- Network Slicing
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:network-slicing';
-- Choreographic Programming
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:choreographic-programming';
-- Modular Programming
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:modular-programming';
-- NativeScript
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:nativescript';
-- Privacy-Preserving Computing
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:privacy-preserving-computing';
-- Micro:bit
DELETE FROM concept WHERE concept_id = 'bokheim:computing-2025:micro-bit';
-- Transformers
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:274df118-288f-56be-b1fd-6dc40ccd2c7f';
-- Patents
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:29ab5fcb-5a12-5d83-9dd4-1205417caba8';
-- Patents
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:deacd61d-4212-5edd-856b-91105be36a55';
-- Flow Batteries
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:flow-batteries';
-- Microbial Fuel Cells
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:microbial-fuel-cells';
-- Perovskite Solar Cells
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:perovskite-solar-cells';
-- Sewage and refuse disposal and drainage
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a615a254-2249-5037-92e9-65f086abd0ce';
-- Water supply
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c6473eb6-4529-5bbe-8353-f2b18f2efb64';
-- Plastics & Microplastics in Soil
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:plastics-microplastics-in-soil';
-- Street sprinkling
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:317c6518-ad51-55bf-8030-eaecabd0ad5b';
-- Laboratory manuals
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e4f9b6f7-80ec-5baa-a334-700e8c2807ba';
-- Symbols and abbreviations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0ca44fc7-a986-57a4-85e7-a88aa677a9a2';
-- Technical blackboard drawing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9453f967-b593-5a8c-aad7-58065647957a';
-- Catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0aa6ad12-a0a1-52c3-846d-be6c3d708153';
-- Artificial fur
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:71b03be5-6e82-59cb-9e04-898ff1da17c8';
-- Catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:343c6750-3848-5057-906b-c283f5dae2c2';
-- Quality control, grading, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3258a53d-15a0-5357-b67b-dd79eb6024cb';
-- Dyes and dyeing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:bfa4432d-8d56-5be8-b8d7-8cc6fd6b1bbb';
-- Leatherwork machinery. Catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:aad767b3-f7bb-5474-bbb2-567f2e29956f';
-- Patent leather
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2e57ef7b-8bb3-51d1-90e1-b727bec32f45';
-- Study and teaching. Schools
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:07dc81e6-f5bf-5319-8947-38b9120aaa36';
-- General catalogs of metal manufactures
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:153b9d12-86e9-5bc2-af21-322c90c41c83';
-- Wire screens
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:69dce33b-1a8a-57d3-afdd-d61c2252bd6a';
-- Handling
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:46ce5702-91c7-5f1c-bbd4-675eb8099805';
-- Protective Packaging
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d91c46dc-d72c-5e4f-a5ff-3f3c5534cfdc';
-- Paper & Pulp Equipment Catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:eeb56003-d844-5caf-b7e5-529e00052bf3';
-- Balata
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c3c46aef-07f1-5de9-9bec-48bbde44acce';
-- Crimping
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c3964b00-d355-5537-b18c-49761dc42353';
-- Drying. Drying equipment
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:390c12d7-f4c3-5cf1-8bf5-5a17fdc26230';
-- Electrostatic flocking
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:48eaa680-2aa8-574c-a004-69d99e140b38';
-- Mercerization
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ec5a0f12-fae5-5729-8f54-c268d80c6eb9';
-- Textile supplies
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6aa67d5c-a2f2-5d39-844b-d79dce4b0755';
-- Umbrellas and parasols
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e8380e70-9dd2-5df1-9278-a78533d40684';
-- Box making
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:99883b13-f043-5572-a6f8-1b54d80025a7';
-- Cork industry
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fc70620e-9072-5ba3-a1fc-4597b09087f8';
-- General catalogs of woodwork
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3b042455-906d-5d79-9411-8a34cd7ee0fd';
-- Wire-grass industry
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b6446e3f-47d9-5373-8be2-6bc490c2e5d5';
-- Machines for caring for plants
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:21d85d09-df9e-5bf4-b0c3-cb7a14a98aed';
-- Patents
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:063f6e54-f94b-589d-8aa9-32f4fd8aeb8a';
-- Trade catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3d35e197-6bec-50bc-ac65-5a36fb99cf29';
-- Alcohol motors
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d50bae50-3318-5be4-bba2-b516b2bb698f';
-- Barrel engines
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b08a0839-acd7-5e64-bf42-329b42c53ae6';
-- Free piston engines
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:831175fd-3d6d-566f-8404-6d4342130109';
-- Furnace-Gas & Waste-Heat Engines
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fcf10356-3644-5989-8d30-efd58eb7f3d3';
-- Gas powerplants
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:45001f4f-f022-55e6-b310-c128192a0ea4';
-- Liquid- & Vapor-Fuel Engines
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0eba087a-91e8-5d6c-b44c-b8c5ccbc9342';
-- Patents
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9b1ead21-dd72-5b0a-afeb-f925d387d41c';
-- Information services
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a70fad10-0130-55b7-a69f-983a59979cc4';
-- Locomotive Builders’ Manuals
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e10ee849-5804-5ed9-9d22-a7017fb8af2f';
-- Locomotive tenders
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f2f14ba0-e39c-551a-801c-dc08c587744a';
-- Superheated steam for locomotives
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:13d64ba5-ce20-5c41-b914-73563082aacd';
-- Catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d922f923-973c-5ada-9b99-9bd191e8ae3d';
-- Water-pressure engines
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9671a8aa-c748-55a2-855e-d7fcdf97299f';
-- Lifting jacks
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f72350fb-a64f-5e4f-9c34-760448f76cea';
-- Testing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9e6fb917-b8dd-56ae-8572-19de55bba8fc';
-- Bench work and fitting
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fb038551-d617-56b1-8b08-c32110173907';
-- Building and arrangement of machine shops
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6a274134-6623-5124-bf60-e895ee55cf7d';
-- Film catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e440dc9a-35ec-5750-ae3b-92f8a162686d';
-- Axles
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2b94af3e-aa1c-5edf-b371-dc29a55385f5';
-- Measurement of power. Dynamometers
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8f13c28f-599e-5d49-8fa2-0a83f5830342';
-- Regulation and control of power. Governors
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3f4fccc3-1ff0-56d9-97f4-1c5c8e89a83d';
-- Torquemeters
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:bfc4ef10-0389-5385-86ae-f960ce1f6479';
-- Soft Robotics
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:soft-robotics';
-- Mine and ore cars
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6c497f5d-e1cb-562c-94bd-8c322f7a9266';
-- Underground roads
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2d1472f1-c1f5-54e4-820e-843856f62bec';
-- Cement materials
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c80a82a6-dfe2-5cee-81f9-7b5d30812dba';
-- In situ processing (Mining)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:379353a1-f423-5260-9d46-e27ea59d50ca';
-- Stroboscopy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1d179225-b79f-5c31-a0b8-0e19ae8fef30';
-- Aerostatics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0c21fe10-1527-5e7a-a57e-d1153bbbadfd';
-- Natural flight
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8e0107f0-c17f-53f4-b946-39f48cbb5b3e';
-- Nuclear Aircraft Power & Propulsion
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:591dc34d-50c1-53b4-aa04-73ed56991921';
-- Patents
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6183643e-4b72-5261-8a1f-62b3adb222df';
-- Radioisotope technology and components
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:afa55605-14b2-523e-9b2a-1470b3321113';
-- Space tools
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:24b34efa-9672-5e07-b34d-d6ff1bb8ed74';
-- Specifications
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a890a3e1-3e97-5f11-88c7-d9217176c6f0';
-- Endurance tests
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:dcfbbc88-cf7b-5996-937c-f44aeb1199b0';
-- Towing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2c009d39-e756-50f4-abc4-cd2da3a30149';
-- Pictorial works
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5df9bd91-1f05-504c-8b85-7b45553298c4';
-- Rocketry as a profession
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3a5114e1-c22e-50d3-b1aa-49ff0aa77ef4';
-- Electricity in ocean engineering
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d2e8db80-391d-50b7-a728-7b77b3ecb55e';
-- Patents
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:94eea460-c9ae-5561-ae8f-811900ec0c68';
-- Biography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f78570f3-8688-5564-9ac4-eaf426c72454';
-- Construction (General)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8ae796b9-bcd7-582e-bdc4-548481229964';
-- Operation and management (General)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:14a34397-2a47-5e3e-8941-40586c7c26df';
-- Car furnishings
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a88f9d79-1257-56bc-af96-8fb57e2b1c96';
-- Car sanitation, heating, lighting, and ventilation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d87d5c46-28f0-5383-b550-26efda90b708';
-- Cold weather operation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2add882a-dcf2-5b45-9bee-659dc12f722a';
-- Organization, etc., of staff and force
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0ce10a28-2113-50f5-8c68-d56490ebfd9e';
-- Biography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:92159a86-4255-5b0b-aeb5-ee9f5ad424db';
-- Chartering of aircraft
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:655a962c-0d58-51ba-9b34-11fb07562e43';
-- Government policy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f8affd60-40aa-5276-b17c-4ad387449859';
-- Rates
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5ed7934e-e6ac-5ba5-a45e-65456a30e053';
-- Biography (Collective)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7c05a92d-86d3-55ba-84a4-476ff5c88cb0';
-- Developing countries
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:af1ca769-cc5c-5e1a-930c-64eac7ecc8d5';
-- Geography. Post routes
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fcf81b8d-f455-5285-a86c-8589dcd4869f';
-- Accounting
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3c5aedaf-fe97-5655-8f54-df183e7b4d5b';
-- Transportation of special commodities
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:cb9e3a52-a37c-51b9-8beb-7bf3d85e8809';
-- Special systems
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:66fff6af-be61-5ad1-a264-9a49f1742a28';
-- Teletype
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:53c24cdf-0a55-5d4e-bee1-a873e6ef2065';
-- Government policy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:993fb025-6d02-5b0f-be4d-49d3b044a476';
-- Statistics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:015fc47b-639e-5bf2-b456-c784d7fe5a1b';
-- Built-in furniture
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5638e9de-df4c-5445-ab73-78c8fb0a73ba';
-- Early works to 1850
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:11f70aac-38b1-5b3d-97b6-534171a7e554';
-- Unit furniture
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:417471da-5436-572e-b09e-29ff107c80c4';
-- Primitive
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6da55ef6-306d-5f2c-837f-c9d51c551315';
-- Trade publications
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:879fbc66-8505-581b-8fed-3cae6bd53c10';
-- Collective biography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:eed6486e-8b6d-5894-8f1c-6ab4b80fa030';
-- North America
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:59187984-815a-57a5-b69e-bb0a6e731804';
-- Dealers' catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4acf0cbc-45c2-57a5-b2c1-355be46af7d9';
-- Dealers' catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:58648b12-76fb-5fcf-9110-8158fd274e26';
-- Museums
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4871d6a8-e524-53aa-a6e8-74a185a9d385';
-- United States
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3cac9f86-a232-5068-ac5e-4c6d0b20ede8';
-- Pantograph
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b2bf6fea-9a42-5b5a-b626-287807e15a8b';
-- Transfer processes
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3f3f66cf-45e8-5f5a-903f-b915d2f53205';
-- Ancient
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:712165a6-349e-5a1a-8d72-86c11c9408b1';
-- Primitive
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:638d9733-6386-5340-9b51-c06ddd471b11';
-- Indexes to illustrations (General)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0230ecaf-bae8-57b5-be57-afb896e591d1';
-- Reflections
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3da00435-b4f4-54af-8a33-3d9d2debc9d6';
-- Dance music
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1e8a50d5-a132-50d0-a24c-5cd8b1a36279';
-- Dance poems with chamber music accompaniment
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:56081197-385c-5e74-aa9d-9fa493301cd6';
-- Dance poems with piano
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5634b9d9-6c2f-58d7-acde-aca87f9b6da5';
-- Dancer(s) with orchestra or other accompaniment
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d58ad4c6-000b-50e6-ac51-ada7f8d613e7';
-- Gamelan
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8dfd1faa-6572-509b-916c-bf9f543d23c0';
-- Radio and television music
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:08c0315d-5e80-5442-b438-1f336fa06bac';
-- Field music
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6d24345e-9746-5c98-b2e5-10b1d3b4a387';
-- Special techniques and exercises
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:62032394-9d7d-5798-9f61-5695f9e9e621';
-- Glass harmonica
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8006bd14-89ab-5ede-979b-a72024f02c19';
-- Implements put to musical use
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:547c8985-bb48-5d47-8f6a-63f6008dbc0d';
-- Phonograph and similar instruments
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b2b87719-12cf-5ad2-b990-f8da9dfd3863';
-- Timpani
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5b14e9f2-02df-5139-b80f-f3a957574bca';
-- Tubular bells. Chimes
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3a17bfc8-fdd5-50d3-86cf-a04b8ce65ac2';
-- Whistling
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e699df46-75fc-5855-a25f-63d8443f9efc';
-- Xylophone and similar instruments
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4483d6fa-83d5-5a8b-b6b8-b696d370a051';
-- Balalaika, domra, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:10b9d46a-f249-52a9-9d39-1ee5ce9b0d6f';
-- Banjo ukulele
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:491054d4-050c-59b2-adde-631f49adce82';
-- Lyre
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c2443954-fdef-5fd5-997a-5b4ccf59c539';
-- Other plucked instruments (General)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6e11e435-e933-5b88-a020-4d95d4a48619';
-- Tambi
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:05b05c4c-f3f5-55b0-8c2e-d6e9c79459fc';
-- Tambura (Fretted lute)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2595d2af-7162-581a-855d-6e2768335345';
-- Ensemble studies for wind and other instruments
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2dee97a3-0931-5907-878f-8f58547d6c07';
-- Ocarina
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:63b52f34-cdfa-5189-b7c1-4c2e1a505272';
-- Saxhorn & Flugelhorn Family
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e3594488-223b-57bc-a496-687f4cb9bb95';
-- Tablatures for wind instruments (General)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0c760936-5d90-513d-a376-72a8c216e16c';
-- Circulars and advertisements
DELETE FROM concept WHERE concept_id = 'unified:b97c8494-af1c-5594-9206-e49121dbf34c';
-- Radio operas
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6f27f469-0bcd-5298-a50e-14ac6d3b7064';
-- Operas
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:48dfa55e-f397-5982-a50e-79c3be31b683';
-- Computer software
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:97364897-2b7c-5c49-8fca-85aeb1874bea';
-- Scrapbooks
DELETE FROM concept WHERE concept_id = 'unified:303412e2-1920-5e4e-a65c-3bc47e35ba26';
-- Law
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5ffab334-e2ff-5f41-8377-3fa55b71362c';
-- Musician Registration & Standards
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e0ad89fa-b66f-512a-bc34-4afd27ecb321';
-- First editions
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8e21c35a-ec9e-5fe5-972f-dbfc564755f9';
-- Copyists' manuscripts
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:818f55a5-2552-5446-b7e9-88dcd9b7ab49';
-- Unidentified compositions
DELETE FROM concept WHERE concept_id = 'unified:e6b41024-2baa-5f29-b68b-c1a3558ba9ae';
-- Manuscripts not assigned to other classes
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:704a977b-196a-5cb3-a33a-042eb5d75088';
-- Owner-Bound Sheet Music
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:47a1e5b3-af18-5333-8ae1-c3af468f314b';
-- Charts, diagrams, etc.
DELETE FROM concept WHERE concept_id = 'unified:630bb604-ab61-5cd4-ae30-db9c5ba7f7f3';
-- Instrumental methods
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e9d94b1a-ee68-5963-a073-3b0e4661d15f';
-- Plucked instrument orchestras
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:85ac2340-5450-5188-9214-935f06d4484c';
-- Correspondence school, etc. methods
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:65a9a13b-07c7-5cfa-9a47-82a8d6c09481';
-- Photo-oleographs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b0806dd0-306f-5707-9ec6-056c32fc726f';
-- Photoenamels. Photoceramics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fb63b49b-04d2-5201-be3c-385f737c77aa';
-- Mezzograph
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:402cfb94-d57e-540c-ae13-c340aaa1dfe4';
-- Photoxylography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:52f87cbe-fea3-5c34-b32d-c6211d319552';
-- Photozincography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:932a953e-e905-5889-92bf-09e5bd40c5d7';
-- Open air views
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b77463ba-0da4-528e-bdbe-bca00d3c3e69';
-- United States
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3af0e4ac-5926-5305-b4f5-1b034b31917f';
-- Astrology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3e2f39f6-f2f0-572c-9edf-a2162a6a6d2e';
-- Cranes (Birds)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:392717cd-6ff3-5841-a050-7cc5cffdd1bd';
-- Doubles
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e4b072ce-4fcf-5afd-9b89-173fec134dd0';
-- Middle West (U.S.)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b3852598-0015-5ac4-805b-b687b7cd20f7';
-- Milazzo (Italy), Battle of, 1860
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1650cb39-de36-54e3-8a86-f4df5e33d340';
-- Minas Gerais (Brazil)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1abc91a6-2622-5a04-bb28-0f11fc83e62a';
-- Months
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:00d02a9f-fd36-545d-b8cb-3ab02b325845';
-- Pastoral art
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:dfa0e837-8d42-566f-bfe3-98d852764fa8';
-- Pitchers
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a8369258-9eac-50e4-8fd7-dcfebcbef848';
-- Roman charity (Legend)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d9381861-cfce-54b5-b07c-07156780f181';
-- United States
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:49c5e75d-ed33-5f6b-aa90-c9633796a227';
-- Mutilation or defacement of paintings
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c6f874dd-bf34-559b-9bbc-fbc17a81354e';
-- Ancient and medieval
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:cc388f80-658d-52ab-9916-01d205e3a1ac';
-- Chronological lists
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:93c39367-49f8-50e0-8d09-3e8ef6b4363a';
-- Ancient and medieval
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8cebea70-6138-521f-82a9-59aad0de77d3';
-- Special artists
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5494097c-d36f-5a21-bfbd-f99bd041adde';
-- Biography (Collective)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d73b04eb-9afd-57d6-9958-9a697c537b44';
-- Collected writings
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f5ebc166-a935-5cce-a3ab-fae641d99bff';
-- Materials for watercolor painting
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8972ba5a-3201-58d4-8699-de36c3371d39';
-- Outline design cards
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:43b1f86d-e5f8-5c33-909a-6a2694f7642a';
-- Reproductions of watercolors (General)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:826c127c-6dcf-5025-98bb-00442ab22878';
-- Collected writings
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:481ecbc1-4251-571f-9cab-8d2ecc2673dc';
-- Collected writings
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b7f182fd-0c6b-5ba5-a360-de1243a171da';
-- Collections in book form
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b42e04d8-68e0-55c3-9622-d2e62bd4a9f5';
-- Instruments and apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9e199a52-2576-5007-b6b0-52ac5d7416d2';
-- Aquatint
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ba5d4f37-f49b-5c2c-b571-2db6e8857d55';
-- Line engraving
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d4878a0b-986a-5b6a-b7e3-8be945ab3524';
-- Metal engravers
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c3c8adb0-4729-530d-8688-c83cd72fb5f8';
-- Steel engraving
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ea0d22de-b070-53e1-bf4b-dcd6c4edee45';
-- Stipple engraving
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f974bdb7-e2dc-5867-81eb-f6c4478ee89b';
-- Miscellaneous minor works. Machinery and supplies
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:686fd382-b846-54bb-b662-341f0c3256f6';
-- Trade publications
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b1a83685-4212-5efd-8c98-31ffdb04c575';
-- Catalogs of special engravers
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:605245c9-9b61-5ffb-b853-8d3a87fac657';
-- Engravings after special artists
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:601fd446-401f-50a5-a28d-301009d27d2b';
-- Instruments and apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:37cd03e1-a8b2-5edd-b5ea-041c375b9fff';
-- Museum & Gallery Administration
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e7ed655e-2161-5dfb-8874-52e74e3c2a4f';
-- Prices of prints
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8fa776db-2b5f-5958-8a2f-36ac5049a9a3';
-- Printmaking as a profession
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d7e23c38-8869-5b52-aa16-d81307f39919';
-- General and international
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:460f2ea5-eaa3-5e27-a8bf-9fc7fa7014fe';
-- United States
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:89ba024f-0b1a-592b-99cf-a428693b3ee1';
-- Instruments and apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4b9d2091-aac3-5edd-bd87-167b4605155a';
-- Intercultural relations in sculpture (General)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a2a379d8-c0a0-5c6f-9572-fe32a2abaa0a';
-- Jewish sculpture
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:696bba7f-c061-53da-b823-1189e2af4f3a';
-- Soldiers' and sailors' monuments
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:eb4083a2-33c3-5999-9263-81adb24c9faf';
-- Wells
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d5f1a9df-44c9-5d6a-b458-2f9edb37d49a';
-- Sign language and braille
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:22cb20b9-7257-5501-85e2-e8d6e63eef7d';
-- History
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a5af50b6-a224-59fc-9905-cf41961fb7aa';
-- Special subjects
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:87cdf9be-6e1e-57e2-b95e-c0e672680ba4';
-- Africa
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8e17d146-853c-56c2-9d32-ea99ab1d5fc7';
-- American (except United States)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3083f940-d761-5bd3-999a-eb57e08d94ff';
-- Asia
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:35b1231b-0bd5-5aeb-b72b-8e9d335c104e';
-- Indian Ocean islands
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7ea18d97-7f34-54d0-8fce-626b802e47e4';
-- Oceania
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d561e2b6-cf0c-53fa-874e-f5a7b8cb3b93';
-- Faroe Islands
DELETE FROM concept WHERE concept_id = 'unified:423fa254-5deb-5eaa-a9f0-fbf32ea287c9';
-- Luxembourg
DELETE FROM concept WHERE concept_id = 'unified:049e9bce-e863-57c8-a298-385a0ea77d7e';
-- Indian Ocean islands
DELETE FROM concept WHERE concept_id = 'unified:5b4a2827-99e3-5b3f-b8f7-bbb44e68e2b9';
-- Arab countries
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:91636852-aedd-5331-96eb-d36de5d148eb';
-- School fairs
DELETE FROM concept WHERE concept_id = 'unified:bd941baa-5ba1-5bb9-b742-49ad85f186c3';
-- Individual institutions
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a022e75e-267f-53de-bb74-1f574c19ca79';
-- General periodicals. General associations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:09238112-0739-5bf1-8ec6-dcfa8f9120d0';
-- Heraldry
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:364b394c-b3f9-5822-aa86-dcda7e8841e7';
-- Inter-fraternity conferences, conventions, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:55b1d8e4-f82a-55b9-afd8-36f20e648f22';
-- Law
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6fcfb1fc-07a9-5e08-b0f9-7d5178101928';
-- Traveling educational exhibits
DELETE FROM concept WHERE concept_id = 'unified:bdde031b-0502-503c-a69f-93734b9739d3';
-- Training of kindergarten teachers
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:290993ed-2dd9-5866-ba8f-f7bf7a2b6717';
-- Academic protocol, etiquette, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b663ebfe-7b29-548d-9014-5fce6a409632';
-- College life in literature
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:68e5fbc0-68c8-5e07-98c0-0ca9d519f3ba';
-- Standardization
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:998ebbee-6557-5381-a5ef-cb4fe4edc9e2';
-- Special
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e2729d6b-c311-5e31-b0b7-a07e37759702';
-- Fire drills
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b97bcd7c-dcc7-525e-8791-fc768b4bdfad';
-- Lung tests
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f51c1147-3c6c-58ff-ac66-32abb38fce0c';
-- Occupational therapy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ea0ca436-e2bd-52ec-984e-46807d09d0e3';
-- School baths
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ec13f843-2e6e-5bcb-8147-6ddfa054a343';
-- School health personnel
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a031ee60-0e32-5711-a3d3-1d7974688950';
-- Cleaning and disinfecting
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:97cce275-6104-53d1-9337-6379ec6b8ffc';
-- High-rise school buildings
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2a4ba000-4e08-541f-9d8b-f7bc1e0a6fe7';
-- Teachers' dwellings
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:41463968-184f-5588-8c22-b3dc89d73f2b';
-- College records, diaries, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9ecc8021-c417-511f-b332-c8899eccb933';
-- Trade publications. Stock control, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c7ee67ab-4e5a-5e94-b3fd-162c1920ad2c';
-- Outlines. Syllabi
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:81694ec7-2c70-51ab-9eab-dfc50b1feb0e';
-- Rural high schools
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:58011102-af9f-5f0b-929f-e613af24604d';
-- Birthdays anniversaries, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:889a6b7b-7aa8-55fc-9b6c-ac3b98ba8e83';
-- Microlearning
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:18a9df9f-5d48-541d-94bb-44a5d0b4b7ec';
-- Practice, laboratory methods, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f1d3fb3b-6a38-5f77-8374-757e9f542832';
-- AI in Education
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:ai-in-education';
-- Polar regions. Frigid Zone
DELETE FROM concept WHERE concept_id = 'unified:12cc5af3-858f-5129-85bc-404fb9cd3497';
-- Tropics. Torrid Zone
DELETE FROM concept WHERE concept_id = 'unified:9983be3f-dc90-525b-8445-d8f1e2ffe09f';
-- Planetary and lunar globes
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0ddc46a8-f0db-5375-b4da-dc971afaac07';
-- Northern and Southern Hemispheres
DELETE FROM concept WHERE concept_id = 'unified:856bcad1-d8cf-5171-8ee2-6f9d474c6cbb';
-- Polar regions. Frigid Zone
DELETE FROM concept WHERE concept_id = 'unified:5f8eb493-c655-5085-8268-d37b7c6c1b6d';
-- Temperate Zone
DELETE FROM concept WHERE concept_id = 'unified:b7f0970e-b4ba-5427-aa5f-f8f667688530';
-- Tropics. Torrid Zone
DELETE FROM concept WHERE concept_id = 'unified:8634483c-f214-58da-89a1-9ebc59e8e70c';
-- Universe. Solar system
DELETE FROM concept WHERE concept_id = 'unified:5e8d6c7e-90e3-553e-afdf-44a1f1cad0f0';
-- Block diagrams
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0ae9984b-82a3-5e2b-9562-93fa48a3a817';
-- International surveys
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:623cba23-393c-597d-89d6-b34f619ed3db';
-- Early works through 1800
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:46ea3ae1-c405-58bc-8b74-53ea6af222cb';
-- Laboratories, institutes, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e958cdde-eb50-55bb-b182-35c2357b93b1';
-- Physicians to famous persons
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0b1cc844-16c6-5935-8a62-bae99febfefb';
-- Acquired disease
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9fe14f63-e3fd-5d4a-ad4c-d21ed5151540';
-- Acute disease
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c226cf54-26b9-5e58-a040-ce46c33b9893';
-- Preoperative technique
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f6ad7fc1-933f-5763-991c-6b38dee84c59';
-- Corns and callosities
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4fd476cf-0503-5e05-81ab-90b9d3b467f6';
-- Anesthesia
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5d54c96a-5550-50de-817b-903411efbda7';
-- Heat rash. Prickly heat
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d4536366-095a-57b9-926e-651351525bed';
-- Abscess
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:613f1df4-3d9a-5635-adbd-324e4cee36fb';
-- Oophoritis
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d457bbd5-4003-5bc5-95b2-5dd11a7a4f8e';
-- Salpingitis
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b080646e-6dad-505f-8cdd-f548896cbd7f';
-- Wounds and injuries
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f489ab39-b31e-5c8b-811f-629a7c58df10';
-- Anatomical descriptions
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9b0754b3-bcae-5222-8036-a9d0a8239b68';
-- Directories of obstetricians
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:98ae8391-3177-57b0-bee3-8332e522d746';
-- Basal metabolism
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:aaf56083-81f6-5239-a7c7-cfa32b44bdc7';
-- Documentation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:faa6b4e9-d3f2-5fd0-a93c-b638815b39bf';
-- Asthma
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:183f2bb1-7a8f-5a8d-90e3-5c303c3a77cc';
-- Croup
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b4c2aea5-cbde-5692-b03a-9b347a4b086c';
-- Empyema
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a153f584-2b0c-5ffd-be9c-13cbd6f75525';
-- Visceroptosis. Splanchnoptosis
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0d452872-a283-5c97-8a0d-f795022344f2';
-- Home care and rehabilitation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:59a47efc-19da-5913-b03d-451eb96f1052';
-- Optometry Trade Catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5d96e679-afc1-5612-be91-9f28f1d281b0';
-- Blepharitis
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f8bc35f6-a57f-5b43-aa00-8b32194dde7c';
-- Dacryocystitis
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:519d181a-d3c7-57b7-b425-4f68abe74b10';
-- Ectropion and entropion
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e4a2f7fa-4a8f-5f8b-9884-4e22bbcac132';
-- Nasal & Lacrimal Duct Obstruction
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4a0ec777-1010-50d2-bfd8-21203bdc6703';
-- Hypermetropia
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:846911f4-5fa3-5957-9974-3bf294a38566';
-- Trade publications
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:90e68369-952b-59b0-bbb1-a1dd7b2376b8';
-- High-potassium diet
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5e22e2c1-21e0-5fc8-bfce-4532cd66914b';
-- Drugs acting on the liver
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f9cca3e8-41d0-5989-b24b-69f988f6e589';
-- Antitoxins
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:78007bb3-87b0-5790-9ec1-1c575a374d95';
-- Laboratories, institutes, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3da25d5c-98c8-5766-8601-de491c765fdb';
-- Protein therapy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3e62eaac-fe56-5faa-a28a-fee8fd946be4';
-- Pericardial puncture
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:704b9cee-6f42-5189-8be2-9783a8274520';
-- Pneumatic aspiration
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f42bbade-6f3b-5099-acd7-6872392144de';
-- Sleep therapy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f2d877f1-b68b-5f3c-94a3-828b0e71a72b';
-- Asia
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a88e97d0-ffd1-5bcd-ae1b-e50aba462044';
-- Australia
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:eddb2537-2123-5195-aa0f-7fb1c57c2945';
-- New Zealand
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:37118096-5221-5578-98c7-24d54f609137';
-- Additions to materia medica. New remedies
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:819dead1-216c-563a-884f-01bda0670d67';
-- Cold
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fe3db5d7-046f-5b8d-8e84-6f13e20f466d';
-- Research. Experimentation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6b029466-7e05-5a5b-b7e2-be7c791cb470';
-- Research. Experimentation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:145ede75-5403-59ec-b526-fc05df3ca642';
-- Wound disruption. Delayed healing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:35bc72ce-8bb2-5fe6-a342-80d00c6471ae';
-- Atlases. Pictorial works
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:954f4363-68f3-5172-b77d-84b8ec0a89be';
-- Surgical nursing as a profession
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:77dc7b55-dedb-5702-bc85-82c08b04be03';
-- Foreign bodies in air and food passages
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:df141fb8-aaae-5736-9252-d3f97452b5ed';
-- Wounds and injuries of the throat
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d6878e23-af17-5b7d-8ba0-1c5c8f295bd8';
-- Complications. Reflex disturbances
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f6d0e06e-c415-59c4-81f1-8a9bea23aae0';
-- Foreign bodies
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7e767570-ee5b-59f1-b67c-fd5a48678f39';
-- Rhinoscleroma
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:848e7315-d7da-5093-afd5-4a4b3119e470';
-- Ecclesiastical vestments
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9ea7f0dd-7b93-5288-95d9-e9dac19d1bb6';
-- Recipes for barbers (for hair preparations)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:232a825e-ce24-5636-9d12-2852bea9a0b7';
-- Blueing and bleaching
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5c35196b-9227-5ad6-911b-4e79b16f885b';
-- Catalogs, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:be5d09de-ce27-5979-8b9f-4c9d2c9ad6d4';
-- Starching
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0cdba489-92cd-58af-9247-25dbcf7990d5';
-- Printmaking & Stamping
DELETE FROM concept WHERE concept_id = 'unified:40fc8881-ef08-5ee0-ab9a-189d69ca7a52';
-- Bellmen, doormen, elevator operators
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f32809a4-eb57-5532-b40c-f5be0e8e9cff';
-- Decoration and ornament
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:aad66ffb-a2f2-54fc-a09f-10e936eecfd4';
-- Ancient
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:327eee79-3c04-501d-ab56-aa00df5c8fd4';
-- Evil houses, stone walls, etc
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2852236f-ca9b-5db4-a78b-981ca676355b';
-- Early through 1800
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:206a58ed-2c14-5417-9293-028a7129925e';
-- Borley rectory
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:2fece36c-6c92-5b32-afc4-1c23e95bdcae';
-- Information services
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2246ee0a-422e-58df-936d-aa79eef0481d';
-- Joint debates and discussions
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8d8a73ac-2e02-5c34-8cdf-4750b781d3ae';
-- Automatic drawing and painting
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:fe55bb5e-7495-5374-85d5-115d66f7a50c';
-- Educational aspects. Intercultural education
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:cd9ffc34-643b-5a06-bec3-1a33ad514d2a';
-- Ancient
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:3fb576e3-cd8f-554c-978d-95b1ecc3bf3c';
-- Music
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:041b0175-08e8-5f57-90fe-768519950c3d';
-- Colonial and Naval Operations, 1739–1748
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-curated:383008db-d8ae-5895-91ce-f7e0535d5b9a';
-- Rhine and Danube Campaigns, 1741–1744
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-curated:d53d8b5d-01ef-568d-9a3b-26658b333f1c';
-- Far Eastern Question
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-curated:b1a876f9-fc40-5012-aa50-9094dcc59e78';
-- Communication of information
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:4f0a99f8-5010-5665-813f-c2e978f7441e';
-- 1886-1918
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:ebcae9f4-695f-5087-9300-eebe6a7824fc';
-- Military and naval history
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:3d6ad0a3-2f35-5429-b6ea-99d3eacaecfc';
-- Foreign and general relations
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:2daaa778-066a-5d99-a0bb-e57111516039';
-- 1918-1945
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:928d0a86-7b75-5757-b6f7-7eff92ea3e3e';
-- British Central Africa. Federation of Rhodesia
DELETE FROM concept WHERE concept_id = 'unified:c5313bfa-aa0a-5688-b61b-d102cbdb582d';
-- Karanga Empire
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:2f49d651-36b7-58b1-aaff-1750c5815eb7';
-- Nguni States
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:4e758ac5-de7e-51a6-896c-11e89964acb7';
-- Rozwi Kingdoms
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:26730d95-d1d7-5d3d-9c7c-8446da641e3d';
-- Zulu Empire
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:d3b60bca-226e-5be7-a283-d16a00850514';
-- Military history
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a9e95753-e43a-5498-8cb6-7c3669b72c3c';
-- Philosophy of Southern African history
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:cdfaad69-7cda-581c-8754-58ec74184020';
-- Regional History of Southern Africa
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:327dd4ee-47e2-5e81-a4bd-df7d73adb1e7';
-- Denkyira (Kingdom)
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:16685043-925b-5c45-bba6-f926a6d341ff';
-- Lower Guinea
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:78628f66-c3e6-53f9-8a51-8c2678ad102c';
-- Allusions
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:cb9bc472-8fe2-5272-8713-801b52a0cdff';
-- Minor works
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:430579c0-bdf7-548e-afeb-5d650b923515';
-- Arikara War, 1823
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:4608b9e7-3527-5c1e-8c40-4d1e1b450c4f';
-- Chickasaw War, 1739-1740
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:62bfc202-2caa-5971-bc82-1498466b4368';
-- Dutch-Indian War, 1643-1645. Kieft's War
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:8626166c-a41d-56f3-be85-6f3b12bb553a';
-- Mill Creek War, 1857-1865
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:708159b5-0fa3-5fe7-9f47-85e78ae460f1';
-- 1884-1945
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:702f480e-478f-5896-9c68-26c23e1a7ec5';
-- Relations with special groups of countries
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc:60a38b87-25c4-5601-9589-e20485378665';
-- 21st century
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:640b569d-320a-5333-918f-91b3f8c415f2';
-- 1853-1868. Bakumatsu period
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:7da2d8b5-dde0-58b9-a3b6-80fa28e23534';
-- 1392-1637
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:d094ee63-c0b6-58d8-af65-9f8872de8295';
-- Through 500
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:3f0efc9d-b432-5f35-87dc-76cb742bcd3b';
-- 1945-1991
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:dfc19323-44b3-52ee-ac8a-46452b087335';
-- Brandenburg
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:d282978a-6084-5065-bc2b-ea4881d5c7c3';
-- Hanover
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:7a6c1484-8999-542b-9c3a-053603683462';
-- Westphalia
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:9f442029-059c-512c-b737-53f8b9d3b8e0';
-- Germanic Period, 406–687
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc:412278d0-af21-593f-990c-33d6161029df';
-- Celts
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:a7877231-2130-5919-9fd3-295f9188fa96';
-- Period of World War I, 1914-1918
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:e45c9747-8c9c-5fdd-b17a-87b38021548b';
-- Early Swedish Finland, 1249–1362
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:1b8daee6-bcad-5e9a-a3c4-4462be4ebf07';
-- Late Medieval Finland, 1362–1397
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:afe26d1b-1d84-5bce-81cf-b1d858bf800e';
-- World War II Finland, 1939–1945
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc:a2d304dd-9514-57ee-a2f4-e2b5e7e06d59';
-- Early Kingdom, 1060–1134
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:ebab7bf3-5234-5a46-b7fa-bae6dd3fd903';
-- Modern Greece, 1831-1832
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:d86ed0a3-f2ab-57ab-9fcb-ce5f994b7063';
-- Beginning of Christian era
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:f03949cd-f3ae-5720-8ee0-d229d5ce60e9';
-- Classical Geographers
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:881b5a66-4374-5d41-88d6-0ebad31c9061';
-- Democratic Portugal, 1974–Present
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:e13e04d9-30d6-50e7-8ea6-85aed98ab514';
-- Second Republic, 1931–1939
DELETE FROM concept WHERE concept_id = 'unified:e2e44309-506e-5eb9-8e69-46a8412d9e3d';
-- Belgium, 1648-1700
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:de4fddae-82f1-5343-bfc3-59b9db1244c2';
-- Belgium, 1701-1714
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:dd29f38e-437d-5f25-b5bb-8acf81475f38';
-- Early Habsburg Rule, 1506–1555
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:d28dfc72-46a0-5eb7-b592-23160ac9cb70';
-- Early Medieval, 843–1100
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:20cef0ba-b271-5686-8e45-cd45e956d697';
-- Frankish Period
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:4afabd33-fe7f-5e88-bc2e-079ad9adf711';
-- Santa Cruz Islands
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:33e6407b-1929-503f-bb2b-a65530465897';
-- Thirty Years' War, 1618-1648
DELETE FROM concept WHERE concept_id = 'unified:e95d8825-755e-55bb-9705-5af556b0f8c6';
-- Commemorative ribbons, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:334087b5-5e7b-58af-9ee7-88e14d15d048';
-- Gas warfare
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:bfd6c021-c8fe-523b-9e3d-8b428e90bda9';
-- Maps and atlases
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:701f2363-b718-50a2-b5f2-f4743afd233d';
-- Syria
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:2145a985-7560-50cb-a4b0-64d6aa855365';
-- Egyptian
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:c5b6858e-189b-5f36-b437-9352a583e427';
-- United States Peace Efforts
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:9eb98ae9-cc4c-54c3-846b-015c61247dec';
-- War poetry
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b8eefe88-0f70-55a8-9704-fa819d83416c';
-- Maps and atlases
DELETE FROM concept WHERE concept_id = 'bokheim:history-lcc-full:a2cd89f0-386a-556c-b15c-a53f1fa98aff';
-- History
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f7f5cc54-06ea-5eee-81dd-373e1e50031c';
-- A - Bantu
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e43a1d7b-68ac-54d1-acaa-a513ad74ec11';
-- Bantu - Sudanian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:abe4231a-b366-59ee-bea9-1de574897d30';
-- Sudanian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f59e5624-ef07-5d1b-a26f-4bd64c998e7b';
-- Semitic
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:899721a3-0dcf-5a00-bcb3-b240a327051c';
-- History of philology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:86efa4ec-8f11-5ae2-97a0-ef31aa0eecfd';
-- Particular groups of languages
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:105910c7-a1a4-5955-ac2d-fda94e2b3c0d';
-- Philosophy. Theory. Method. Relations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:959fea9d-51b0-56bc-a419-ea3905234084';
-- Etymology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:14a9e1a0-a1a5-5bfc-973e-7c4a766b5ad1';
-- Punic in Plautus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:18b52c21-1a18-51ea-8be1-c5c02dd6e64a';
-- Unclassed languages of Asia
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f66439c5-2869-500e-aab3-6acbc3c8d72e';
-- Linguistic geography. Dialects, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b40639bc-c6e7-5fea-a060-e96a883f9fbc';
-- Gadabā
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:550d8e67-827e-5d9d-babf-2e81a9c69fa9';
-- Kisan
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a32ea877-4658-5751-be15-0e4f9e9d9c9a';
-- Konda. Kūbi
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:75c0599b-3c35-5942-9f19-1a401d49ffed';
-- Koya
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:78de7914-670a-597d-8730-84437299b542';
-- Kōta
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:18fb0c99-1b6f-578a-a179-078242727786';
-- Parji
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2192a338-003b-51fa-8959-80ad5ac050f4';
-- Romany (Gipsy, Gypsy)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2a225bb0-70fa-59ad-b6e6-182aa9246650';
-- Yerukala
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6f57c3fa-0d79-55ce-9b4f-926b17b34368';
-- Grammar
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8fbe4856-2df8-55fa-b4b2-063038d86cb3';
-- Literature
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:eb12b439-806e-53a8-9ae8-b94097d68443';
-- Atlases. Maps
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6b73fb1e-88f7-5de6-adc3-a0fe542e0951';
-- Biography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:35cb018b-1156-5620-985a-1547d101d270';
-- Indo-Aryan Regional & Etymological Vocabulary
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d9d5442b-623a-533c-a4dd-3e7555cd5994';
-- Lexicology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:88ad71be-61ac-59f7-a009-a54bea5f3f43';
-- Style. Rhetoric
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b6330b46-50d4-5e22-9038-e94e05edcd2f';
-- Translations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1208bc91-b0e9-5b82-9db2-ee58395632bc';
-- Translations from Prakrit into other languages
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7bde5c29-9b24-55f5-96dc-f07fad087e71';
-- New Persian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8a224ec2-4de0-5cc3-95ce-3da0a8661709';
-- Outlines. Syllabi. Tables
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5119da6a-28c7-527e-936b-b5f589b077d0';
-- Encounter
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:a5191f01-c2f8-536f-b169-b77d85c3280d';
-- Political ethics, citizenship, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:0159e7bc-48d1-5db8-9db9-5346adb2f271';
-- Bolivia
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:aedf288b-c95a-5c45-83c4-c5d49c15da49';
-- Central America
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:c0083107-8c6a-58d6-9793-4c1752f8453a';
-- East Germany
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:887841a4-7cec-58bb-bee4-448cb0d0ab6e';
-- Ecuador
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:14f8ea37-0b88-54d8-a50a-e54c14984f87';
-- Greece
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:406cab9f-e58a-5ec1-a1ae-2ce45911a5d1';
-- Guianas
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:15011566-d2b4-5dde-8071-22984dc30c7b';
-- Low Countries
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:71f54bad-898a-5169-85a3-c090ec2f969e';
-- Mexico
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:ce85e8cd-7791-55b9-96b4-6d0d7a41fd53';
-- Paraguay
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:221d0ec7-49ab-5a13-ac88-0b6c13a23cd3';
-- Peru
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:66100cfa-bcd2-50ee-9345-90c4a9358f53';
-- Portugal
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:2bf9a2f8-7b62-57d7-8ff9-94d45b1a26c4';
-- Spain and Portugal
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:9c221cce-742f-5d2a-bc3b-df2baea4d084';
-- Uruguay
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:d2e30f96-5d5c-59c2-bc6c-d80aa09a049f';
-- Venezuela
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:501ae01a-3698-5fc3-989f-4780468cd055';
-- Assyria-Babylonia
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:5393f804-5b72-5aae-a2bd-67abf00eb5c2';
-- Italian
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:027d262a-973b-5115-801e-36d8c4b860b3';
-- Russian and other Slavic
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:cd50dfe3-8d08-58ac-a900-a1b7beb3e6b7';
-- Rome
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:4ec278d8-29a6-504e-ab77-d6643ac3274a';
-- Stoicism
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:f5f4a2c1-6a02-5f6b-b763-3337f1255ec2';
-- Dogmatism
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:3536c5d3-530b-556f-98e3-35115436f08c';
-- Meaning
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:4cf78ad0-ed68-5e50-8630-2fba0ec4c169';
-- Moralists
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:1e0ce67b-5b75-5620-8df3-948b3e1ce6e3';
-- Neo-Pythagoreanism
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:12b30921-a235-5491-9d11-df21c726ab28';
-- Elean-Eretrian school
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:e3402be4-3b44-5d4a-a957-f060bfde9e03';
-- 17th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:9c6b102d-ebef-5f53-b3fa-809f4977eecf';
-- 18th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:d625c638-bb1b-5386-9731-35f4cf4755e3';
-- 19th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:5e41339f-549e-56b7-b817-aaace80ee388';
-- 17th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:fdfa3383-b600-51e2-889f-1932126ec250';
-- 18th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:73ef7cc1-74bd-55a6-a174-cba0fb68059f';
-- 19th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:37897e81-c415-527c-bdac-da532c35c945';
-- Cambodia
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:0ff33e3e-36ed-5c17-9909-36aebe3dc532';
-- Laos
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:2fd5c83e-20fc-5036-b9a2-34f0712c6fd7';
-- Jordan
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:8b5de55f-d4fd-55f5-8767-36c65910802a';
-- Saudi Arabia
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:9b334d33-afc7-557d-9323-2496e9b4ee02';
-- Afghanistan
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:dd2dcec3-7e6e-5431-9b84-2f75d707d702';
-- Thailand
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:17600ca8-db95-5cfb-bbc1-3718bae85c97';
-- Mozambique
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:64083a30-d69a-5cea-9971-9bb1e48109fa';
-- Somali Republic
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:d5003d65-3181-5f77-b644-d2511976e7be';
-- Sudan
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:11488d1d-508a-557e-8e40-730773392aab';
-- 17th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:b05b515f-7499-5562-9c8e-668a220f53d5';
-- 18th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:f4e229da-4f94-5313-be85-46098145e7cf';
-- 19th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:ab795547-8f7f-565f-a9a7-5330478dc2dc';
-- Armenia
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:1b983129-7334-5f37-b7d4-0fec8cbde411';
-- Macedonia (Republic)
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:63448fd6-8e2f-53e4-927c-571419cc3da9';
-- Monism
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:256bb43e-d65a-5954-859a-64d3beb36d1a';
-- Guianas
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:0386a9fc-16d1-5afa-be7a-cbd630332975';
-- Libya
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:3b9b4e45-9660-59f1-83d1-03f6d231cf02';
-- 17th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:1f667365-11e3-5736-8b4f-9972a2b2b049';
-- 17th and 18th centuries
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:715ef359-20a2-53c6-a7ef-9656c962aa23';
-- 17th century
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:78879ffb-2880-5385-b142-d1cc4edbb236';
-- All (Philosophy)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:20a36c95-668f-5095-87ef-ffc5c21a8788';
-- Number and quantity
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:77b35554-5fe2-5a5b-80cb-9d054dbd3109';
-- Space and matter
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:f7337bfa-aefb-5d0c-b3be-df7a541e3307';
-- Club etiquette
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:56f45321-383d-57cf-ba05-3d2a30567f1a';
-- Etiquette of clothing and dress
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:7fd9084d-0456-51f2-9b55-c767e1565170';
-- Dance etiquette
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f7835797-5101-5ec4-b15a-ab1d000c5d9b';
-- Catholic
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:92fc8c2c-bb18-5b93-b316-be87c64a4b8b';
-- Arctic Religions
DELETE FROM concept WHERE concept_id = 'bokheim:religion:arctic';
-- Indigenous Religions of the Americas
DELETE FROM concept WHERE concept_id = 'bokheim:religion:american-indigenous';
-- Pacific Religions
DELETE FROM concept WHERE concept_id = 'bokheim:religion:pacific';
-- Generalities: Serial publications, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0207f130-dd80-52ca-b05d-98abccc259e6';
-- Textual criticism, interpretation, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:36821b6c-24af-5210-b192-f86856526e12';
-- Theory. Methodology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:af92f002-a575-51a1-bfdc-12a7d2b70ad3';
-- Dramatizations. Fiction based on Homer
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f9a1af75-7ad1-54a8-b4ee-467b1c6abb4a';
-- History of Aristotelianism
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e9566b6d-2ce2-508a-8060-3612bce5341a';
-- Parodies. Travesties
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:033d4ddb-411f-57db-a2f3-5a33ff727a7c';
-- Popular and juvenile stories
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c1f20a08-9186-5099-8113-edbf2b75ff9d';
-- Selections. Quotations. Passages
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7a636a91-ff00-57ed-8b12-07ff3313a1d3';
-- General. General special. Special topics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c14c9f82-b819-5bdc-9f67-31e9a0f5ce27';
-- Lexicographical
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:35847162-621a-528c-9899-e075e0364bc4';
-- History of Literary Criticism
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6dce0073-d836-5245-818e-73727ef38363';
-- Iconography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:afee19bc-c8f9-579c-b602-33a443377bbb';
-- Literary landmarks
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ae7a6965-a6f4-5de4-97b0-8bf354733d5d';
-- Literature’s Relations to History & Culture
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:020d0b73-0322-5c2a-a874-9333eae17066';
-- Local literature, by region, island, place, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:357edf83-0670-540f-b462-31813d89bc31';
-- Outlines. Quizzes. Tables. Charts
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:31afa7b2-e985-5957-a43d-d7f6aec13a23';
-- Textual criticism, interpretation, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5d02a9b8-8d13-52b6-a4a5-7945e6573555';
-- Treatment and conception of special subjects
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0895b8c5-258d-5c09-a3ff-ea76bf225796';
-- Treatment of special classes
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fa5c0731-84c8-5657-af0b-a9e88b7167ba';
-- Latin
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:22e5fde6-e357-555a-b449-a8a6cabbc344';
-- Polyglot collections and selections
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:26edb679-8ba0-514e-94af-13865c05f61f';
-- General; General special; Special topics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c6de0016-60ea-5f73-a0b0-7ef503f193f6';
-- Representation in art
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c4ecb644-3021-526a-85bd-b81ca42dc64f';
-- Textual criticism, interpretation, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:74b941c0-d6fe-5f68-bd3d-3c2a8f114f12';
-- Polyglot. By date
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ece768cd-a62b-5658-b09f-e8176564a246';
-- Foreign-Language Translations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8830cf4a-3b1c-5fd0-9e84-c4fb76a436a8';
-- Chapbooks
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:18438bfc-c0c8-5559-ad38-d85265909b50';
-- Translations from foreign literatures
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a131e291-5379-56d4-8ba0-c979fb9d0b96';
-- Latin literature of the Anglo-Saxon period
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:210e4a8f-1ee6-532c-af4b-ed1d4d628d04';
-- Collected essays
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:abe9721e-82e6-599c-81bd-8c7de3ff3e21';
-- Communication of information
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:81354116-d45b-5900-bbac-398bbdd5c21d';
-- Foreign literature by English authors
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:43c73085-98df-5835-9be1-d52c358dc117';
-- Philosophy. Psychology. Aesthetics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8d534494-dbb2-5d77-9485-c61c3443ee36';
-- History and criticism
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8a8fa543-d173-57e1-a42e-ce95c2d7af2d';
-- Flemish Literary Theory & Study
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2cfc73c3-856d-5677-9e4b-524cb74b0dd3';
-- Philosophy. Psychology. Aesthetics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:94199ea5-c2b3-5957-8ea3-6de27f294ee8';
-- Individual tales
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a49434f8-5d7b-55a8-89f1-a60eb927c065';
-- Pictorial atlases
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d8b71ac8-001a-52e6-8b3b-2372fb287d1a';
-- Folk literature
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6dc582ee-d357-5c35-a5e5-7bc718f3acdd';
-- History
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4d456a0d-f8e9-5b7a-9861-5324bf0a0b40';
-- History of literary history
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:386a6586-838a-5b1e-ae31-7e1e5a2d6b3a';
-- Philosophy. Psychology. Aesthetics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:16cf9884-82a7-5256-aa07-de1ccafe0510';
-- Societies. By place
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:52940313-1daf-5107-b378-627b2fa6b55d';
-- Local
DELETE FROM concept WHERE concept_id = 'unified:99296459-ccd9-531c-812d-b98266d43035';
-- Anonymous works
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2a05bbe3-05ae-578f-b6cc-5042dd69580d';
-- History
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:781386bb-7ffe-518c-a57f-239f62be7ee7';
-- History of literary history
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0d819666-abd3-5f50-ab7f-0ae2c3696360';
-- Norwegian Literary Theory & Study
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b6bed5b9-8844-503f-ae4d-237e6ec70df6';
-- Philosophy. Psychology. Aesthetics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:78aeae86-9917-5ab8-8e5a-92863d6ed4ff';
-- Biography of teachers, critics, and historians
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9f1f6d5f-27a8-5607-a5ad-7eba5763f261';
-- History of literary history
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7d5f17c6-9657-55b9-bd17-13a60e6dd57b';
-- Philosophy. Psychology. Aesthetics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b6fd8cd0-21d1-56a5-8a9e-4dd7d63c9dec';
-- Icelandic Homilíubók
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a9140826-1c71-5604-a1bb-fd32625826f7';
-- Norwegian Homilíubók
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:510c2461-c72d-5914-8daa-1ed00d8ae435';
-- Postola sögur (Legends of the apostles)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5c90d68f-b732-5ba7-912d-c66b414f843a';
-- Philosophy. Psychology. Aesthetics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:cc31d501-9b5a-5c45-a663-75fb23403063';
-- Individual tales
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:22d62dba-7499-551a-91b9-c24b24c4e2eb';
-- Special characters, heroes, fairies, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d7a7943e-4e0c-599d-9da7-9b543691cb29';
-- Treatises
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:cdcc10f4-fc6b-5996-b625-40d43ecfb5eb';
-- History of Italian literary history
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1a49770a-b450-57d5-bb95-3be26bb34b73';
-- Italian Literature Studies
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:dbfe4b30-4bc3-5e31-848d-2c1add323990';
-- History of literary history
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9e3952d4-51a6-50f4-b349-22020679b951';
-- Texts. Sources, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d5a2dba6-ef0b-530c-933a-e23010ca29f1';
-- Languages of the Montagnards
DELETE FROM concept WHERE concept_id = 'unified:d0942d3b-0a70-5c1e-afef-8b1368fd3bc0';
-- Meditations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0fe0400f-ee68-5cd2-8e34-61c244ce8e64';
-- Uneducated poets
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:eba15715-136c-5eb9-b371-1383922327c7';
-- Miniature theaters
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:86597e68-7c9a-5369-b967-595b903da40b';
-- Goat pack camping
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6c720428-85a1-5e26-98ba-d2ff8d01d9de';
-- Carthage
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:219ae34d-ec8c-5f2e-85f6-ffe0b0db8304';
-- Former Italian Africa (Collective)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:01ae0597-0d3f-5230-99fd-5b4b42945e6a';
-- History of bibliography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6bcfe4d1-712c-5cc6-81ed-d1513e17f18e';
-- Ruanda-Urundi
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:62969386-b489-58a3-91d3-8abc3dc65ec3';
-- Latin Orient
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f962ce57-c3f7-5274-92ef-5cf8cdc417d4';
-- Liechtenstein
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0921c873-35c8-5a77-9ae0-cbe60fea756c';
-- High-priced books
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:6ceae81b-986b-5795-ad29-384abc7a5075';
-- Cotton
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:cotton';
-- Fishery Technology, Fishery Processing
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:fishery-technology-fishery-processing';
-- Wool
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:wool';
-- Riddles
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:riddles';
-- Letters
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:letters';
-- Symbolism
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:symbolism';
-- Bells
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:bells';
-- Stuttering
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:stammering';
-- Atmospheric Radioactivity
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:atmospheric-radioactivity';
-- Cosmic Physics
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:cosmic-physics';
-- Almanacs
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:almanacs';
-- Ana & Literary Miscellanies
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:ana';
-- Flagellants & Flagellation
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:flagellants-and-flagellation';
-- Law
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:law';
-- Radar
DELETE FROM concept WHERE concept_id = 'bokheim:subject-bibliography:radar';
-- Playing cards
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:223ee5c8-f4bb-5de3-b5e3-434715c3606d';
-- Sales catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5af1cbd9-d7a4-59d8-9dc6-bc23317b9727';
-- Printing of documents. Public printing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c4f5fe75-cb6a-5506-8267-a31f6cca294d';
-- Psychological aspects
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7ae5bfee-cfb3-5040-9a5c-4cca9836dbea';
-- Security printing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:21642279-240c-51f9-b334-7e8a9abda2c7';
-- Medallic history of printing
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ececf99f-01c9-5780-bcbb-05ab36145980';
-- Facsimiles of particular manuscripts
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:81b86d36-5a47-55ab-a393-1511d59da0aa';
-- Afrikaans
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8067368a-9ff3-5705-9306-014b2de8638a';
-- Assamese
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:08515f5a-c5d5-55fc-b435-ee9eebf8e369';
-- Austrian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:693c7215-b9c0-5378-be25-91f8d83fd2ec';
-- Azerbaijani
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:bbb98749-8ff5-59db-97a1-d3f7f0995d89';
-- Basque
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:faebe58f-763f-582a-8d58-152c41b7fc6e';
-- Belgian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1fe5b7a6-72a4-5a15-9a69-e428a06e81d3';
-- Bengali
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0b937fd3-6222-5306-abbb-4262e92fa7dc';
-- Burmese
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c5178e96-1ff1-567d-9509-b5794b8a4b66';
-- Galician
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:091a7501-4088-5fcf-bb41-41cb68b0e399';
-- Gujarati
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5375193d-edc5-52a7-97fb-4a09e08a9ef5';
-- Javanese
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4525f893-e4c6-573e-b9cf-f963a3286421';
-- Kannada
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1c6a82cd-0b55-5eb2-9c29-6640c30e7e82';
-- Kirghiz
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fb088450-b4c0-5911-9d81-c08e9a26d16c';
-- Kurdish
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:85e1f952-5045-5f91-975e-9e4997329538';
-- Malagasy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d1355c55-73a8-5f78-859f-4e571719917a';
-- Malayalam
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fba1776f-aefe-50b2-8eae-e7bd51d1f216';
-- Marathi
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2582854e-e419-5751-8794-08e60a4c2ae8';
-- Moldavian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2a4f3ac3-0509-5281-8ec8-4c58ee7bd2c7';
-- Nepali
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c837082a-66f9-5146-b085-78a6deabd3d9';
-- Panjabi
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:30450004-eb1c-56b2-a232-fbfc7579f825';
-- Romanian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f47637d3-8fef-5808-97ab-6c1db175f94d';
-- Sinhalese
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a6b22ee6-ac24-5e83-ac66-5926446fe855';
-- Swiss
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:415e5706-d8ca-5bab-81e0-d3596f06eb25';
-- Tajik
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:49a64da0-f744-5576-bc13-802e8ce77516';
-- Tamil
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2be7d18f-3d65-5dd6-87ef-d09a906b609c';
-- Tatar
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:14f7f875-0cee-5e85-85b2-c2e89080237d';
-- Telugu
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:48244970-b2e3-5c59-b9fd-349466937758';
-- Turkmen
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1b742eef-87b4-514a-ac66-5b870dac03d4';
-- Initiation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a571d94a-1e40-59d1-84fb-2aada1d0a50e';
-- Privileges
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d3427e79-99e0-59ad-a5e5-039ef12d27f8';
-- Monograms
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c0077bf6-12c9-54e5-9092-c026d6a57521';
-- Conservation and restoration
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ef673876-fd45-578e-a47c-b52205a299ee';
-- Spanish-American flag
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ef042529-85cd-5aa2-b717-1d649d338ffa';
-- By race
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1501b1d6-5dc7-5fe8-9b84-9a2986f08e24';
-- Order of precedence
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:bedf2ed9-21b7-54f6-852d-24c2a218ef53';
-- Iconography
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1d821532-dbf7-540f-a064-e57283a21a04';
-- Renaissance
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:da026fce-3a0c-52e8-9b33-52e4a3559fa7';
-- Sales catalogs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0f862d25-6c40-51e7-a65c-4d9cf326344c';
-- Specimens
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c1561273-4c5d-5592-8a0b-ab93c576a7eb';
-- Filter Bubbles
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:filter-bubbles';
-- Information Ethics
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:information-ethics';
-- Information Overload
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:information-overload';
-- Atlantic Ocean Islands
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f34ed80f-c0fb-5330-a32f-b72bfa499a9b';
-- New Zealand
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:968a8888-4130-571d-8f3c-1a8c3b4a4787';
-- West Indies
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:96ae14d5-386d-5934-93b6-a9081381c5e1';
-- Bermuda Islands
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d4235d98-cb51-552f-8856-3a78816ac6c5';
-- Early almanacs
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ca12627b-5967-51ef-b7be-22d022fe04d9';
-- Architecture
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:07b40908-9993-55cc-8c92-5ee0f3e19c24';
-- Collections for Jewish readers
DELETE FROM concept WHERE concept_id = 'unified:e15e0f5f-d0a5-5ba7-8f99-c54092347004';
-- American (United States)
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a3b5d3b7-7c37-5502-b065-202208fea597';
-- Belgian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:213dde72-2167-5d5f-bc21-48f835b88b1f';
-- Dutch
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:aa7dd0dc-ada9-5fb7-97b6-3ee03c54c248';
-- English
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d818dd3b-a6bb-5867-ba5d-a660399710c1';
-- Finnish
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0b84cabe-ab61-56ac-9143-770ca7a23857';
-- French
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3ab02da6-647e-5b9a-ab17-03bc2c2ef6e8';
-- German and Austrian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e243f12c-74b1-5e52-bf37-2a005e57af2a';
-- Hungarian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4678f4e1-b358-5901-8daa-d76796e56801';
-- Scandinavian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d6e84f52-3d7d-5d06-bbfa-0442752c0362';
-- Slavic
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8c8dce04-f77a-5c0a-8f13-30e3eca557a3';
-- Swiss
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1e79e0d1-7bb8-5287-8502-7162d06c10ed';
-- Dutch
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:1ef30e23-605a-5665-b0e1-a4af4c889968';
-- French
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:8e04277c-bcfe-514c-8eff-ccf9df7f89a6';
-- German
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0e69732a-b071-5fdb-af2c-67bb422222d8';
-- Italian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b85c660b-78bf-5f3e-bfe4-a97a537f2157';
-- Scandinavian
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0143d58d-2f2c-54ea-8a4e-49842c7afaca';
-- Slavic
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:31e56a81-13b0-5559-a803-cc678d43f14d';
-- Spanish and Portuguese
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c80ceca4-75c4-5d5f-acd5-c75cf535c29b';
-- Scrapbooks
DELETE FROM concept WHERE concept_id = 'unified:26edf17a-28cf-5953-9be9-2967d1624904';
-- French
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:787ab9d6-8022-56aa-8dd5-c393012615e5';
-- Northern sky
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5917a93e-6035-58fe-86e5-32499b9b65a5';
-- Geodesy as a profession. Vocational guidance
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2e539313-4914-5448-8b40-37dda83acee7';
-- Radar in astronomy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:eef138da-2a03-53f8-ad78-9baa6d5df5d0';
-- Determination of the ecliptic
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c30ce351-a686-5460-a66d-3e210cb26ba0';
-- Position of pole
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:05c793cd-1312-529a-a9e9-996c93238283';
-- Induced Pluripotent Stem Cells
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:induced-pluripotent-stem-cells';
-- Paleoecology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f770c0d2-aa8e-55ff-a659-b848dbfeb034';
-- Laboratories
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:56360dbf-b467-5cbd-9a7d-8f2943c6cc71';
-- Accessories and their use
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0dc5fd44-c3ec-571b-b23f-c40041f73773';
-- Aquatic microorganisms
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:396e210b-a1e0-5771-aa3a-081a82a9d655';
-- Illuminating apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c913447e-7769-5468-95ca-c7e2c6b42154';
-- Alternation of generations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a3540b28-1b08-579b-858b-ddad1d9f350b';
-- Anatomy, physiology, etc.
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:87f0c2b7-e76f-5362-bd89-325905dae0dd';
-- Alternation of generations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:82c10ef4-543b-575e-915b-642a2ec10722';
-- Individual plants and groups of plants
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:427118d0-2ff0-5c84-b95e-43c4f1edc1ad';
-- Ecological discussion of areas
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b9077827-76f5-558c-a2f2-68aef092f4b5';
-- Early works through 1800
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:f3b800ee-89f8-5dcf-8b7b-51e3b0278556';
-- Instruments and apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:9c723019-a74b-5549-991a-d8d943fc5da9';
-- Miscellany and curiosa
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:940147b5-d3fa-5ca5-b7f7-b3079c45c0b6';
-- Early works through 1800
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fdd1e557-4008-5fd3-afb5-25688c3546f3';
-- Electric furnace operations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a7eac8b5-887e-5f0d-b2b9-6a07af696e89';
-- Classification
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:40226e92-1a83-5cae-9bfe-f3ef498134ce';
-- Allotropy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:fc57bf58-1a4f-5e3b-9a3c-0e463c0e8767';
-- Temperate
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:95c96141-681b-554d-a5c7-f70f05103abb';
-- International Years of the Quiet Sun
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:abd1b6f5-f5a6-5945-bb32-13ecb1f41ce3';
-- Instruments and apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d427ec34-55b9-5d81-a64c-acc58cbcce73';
-- Wetland hydrology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:ca85cfc7-6ce0-5fe8-98bd-3d9e25879d25';
-- Leucoxene
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:54f653d4-afb6-522f-9cf2-c940a2822068';
-- Coal balls
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:b2b43794-a378-56fa-982f-8fbafe0675fa';
-- Pictorial works and atlases
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:22690ce8-424c-549b-98cd-7f615eddbc9f';
-- Plants of unascertained position
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:7abb8914-b4ab-543f-8539-3592dc02b373';
-- Instruments and apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:87e8283a-6434-53a7-a05c-3518bad6f917';
-- Catalogs and collections
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c5459a8b-2ebf-531a-b6a1-aa354e296a6f';
-- Classification. Nomenclature
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:e6ed2a9c-b002-50e0-a852-8d123062fb82';
-- Pictorial works and atlases
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4bf0b714-731f-51e7-9a5b-da6700b681d0';
-- Immunotechnology
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c08c2b95-cba5-5617-8535-85e5ea04c2ef';
-- Laboratories
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:2ef9dd8c-c2f5-5e8a-8bdf-932549dd9844';
-- Lysogeny
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:64b9e4a3-0c73-5ada-926d-61d4e4b0307e';
-- Terminology, notation, abbreviations
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:cb0ffcff-be49-56b6-8535-ceebd3f6af44';
-- Virus-virus interaction
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d8c0b103-cdd2-5862-8413-45fb513b6690';
-- Early works through 1800
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5c6d6892-4e59-5a0a-90ad-bd551c6cdc8b';
-- Laboratories
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:c190b0b4-38d4-5d5d-9ba3-b96cd7f041d5';
-- Laboratories
DELETE FROM concept WHERE concept_id = 'bokheim:physics:electricity:laboratories';
-- Philosophy & Theory
DELETE FROM concept WHERE concept_id = 'bokheim:physics:electricity:philosophy-theory';
-- Thermomagnetism
DELETE FROM concept WHERE concept_id = 'bokheim:physics:magnetism:thermomagnetism';
-- Instruments & Apparatus
DELETE FROM concept WHERE concept_id = 'bokheim:physics:magnetism:instruments-and-apparatus';
-- Philosophy & Theory
DELETE FROM concept WHERE concept_id = 'bokheim:physics:magnetism:philosophy-theory';
-- Miscellany and curiosa
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:11a026cd-c8a0-528b-932d-439b2807cdc1';
-- Miscellany and curiosa
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:00d9a7df-2ad8-5783-be8c-06274b240590';
-- Optical constants
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a6ee5425-0d12-5959-85d1-b0f754aab88b';
-- Pictorial works and atlases
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d91c14b5-89f1-58d7-a626-9f0a6187d707';
-- Magnetohydrodynamics. Magnetohydrodynamic devices
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:27ce79f9-135c-5b89-a271-c9da4f5187c4';
-- Collection and preservation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:90ef957a-7c7c-5af5-94bc-0fafa1bdcd58';
-- Animalcules. Early works on infusoria
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:757672c0-16bb-522c-afee-905b1326aa47';
-- Feeding and feeds
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:0b870be6-603a-5e51-bba5-fb2791113936';
-- Quality Control
DELETE FROM concept WHERE concept_id = 'unified:4258aa53-e461-55aa-93f1-7676e0383420';
-- Criminal Procedure
DELETE FROM concept WHERE concept_id = 'unified:e483bdb0-2d52-5464-b772-3f36c61dbc07';
-- Philosophy of criminal law
DELETE FROM concept WHERE concept_id = 'unified:764d9569-3ce9-5fdd-af87-f1f56c0c6622';
-- Tax and customs crimes and delinquency
DELETE FROM concept WHERE concept_id = 'unified:13206383-7091-50c9-97cc-a27cb353fb3c';
-- Tax and customs crimes and delinquency. Procedure
DELETE FROM concept WHERE concept_id = 'unified:559aa039-fd3e-5411-9d7e-02590b7d4b59';
-- Insurance Law
DELETE FROM concept WHERE concept_id = 'bokheim:law:comparative:insurance';
-- Maritime Commercial Law
DELETE FROM concept WHERE concept_id = 'bokheim:law:comparative:maritime';
-- Negotiable Instruments
DELETE FROM concept WHERE concept_id = 'bokheim:law:comparative:instruments';
-- Foreign relations
DELETE FROM concept WHERE concept_id = 'unified:23a26886-3c55-5e51-8228-421b5d4d4688';
-- Treaties
DELETE FROM concept WHERE concept_id = 'unified:8ca28b86-f9a2-58df-bd41-74645a15449e';
-- Technology, Media & Communications
DELETE FROM concept WHERE concept_id = 'bokheim:subject:law:regulation:technology-media';
-- Church and state
DELETE FROM concept WHERE concept_id = 'unified:b2565b66-1f07-57db-a492-e4dff3d8b13a';
-- Religion and state
DELETE FROM concept WHERE concept_id = 'unified:d7588d6c-c31a-5543-8660-45d3c4479e0a';
-- Naval Forces
DELETE FROM concept WHERE concept_id = 'bokheim:military-studies:naval';
-- Information services
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:a870d29f-8beb-5661-8de8-8b4063bb743e';
-- Relation to medicine
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:f452da89-e427-5f8d-b0ec-0b6638334869';
-- Relation to teaching
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:a57bca81-cb1c-55dc-be2f-9788806227da';
-- Language. Authorship
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:56f5b82a-2b0c-55c2-90e1-81185be77796';
-- Motor ability testing
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:77cea148-0ca4-5221-8b94-5ae203503f07';
-- Fear of Missing Out
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:fear-of-missing-out';
-- Disorders of the Will
DELETE FROM concept WHERE concept_id = 'bokheim:philosophy-lcc-full:2cd279e2-afa7-5e58-9892-cb64fce8b62d';
-- Class psychology
DELETE FROM concept WHERE concept_id = 'unified:fb137946-ea63-552a-be9a-158cbbf4075f';
-- Terminology. Abbreviations. Notation
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:4cb736d2-e2bd-523f-9f3f-2bed212af151';
-- General works. History
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:27c2c559-2da4-576e-933b-2dc16d723654';
-- Official documents
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:dbc11d50-ce2d-53e4-beb0-930a689430d1';
-- Alcohol and and people with disabilities
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:a7f044a5-7d1b-5f7b-ae0f-b43efd74bf60';
-- Alcoholism and the navy
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:3ae816ad-b0ad-5632-9965-5d02bf46be2a';
-- Alcoholism in colonies and in the tropics
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:d8395c42-3b4e-5625-b872-570a6a4131ed';
-- Light wines, beer, etc., and temperance beverages
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:91de9c73-ca47-5017-bb9d-8ec3d98848d5';
-- History
DELETE FROM concept WHERE concept_id = 'bokheim:lcc-detail:5a0d9dca-4540-5004-8c98-6cd2f6273bb8';
-- Online Identities
DELETE FROM concept WHERE concept_id = 'bokheim:topics-2025:online-identities';
COMMIT;
