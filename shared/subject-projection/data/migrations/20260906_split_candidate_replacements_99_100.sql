PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Replacement for 51 (Labor—General): Discrete Mathematics has substantive selector families.
INSERT INTO concept VALUES ('bokheim:discrete-math:graph-theory','Graph Theory'),('bokheim:discrete-math:combinatorics','Combinatorial Analysis'),('bokheim:discrete-math:coding','Coding Theory');
INSERT INTO concept_parent VALUES ('bokheim:discrete-math:graph-theory','unified:21b67f0c-588c-5f8a-bfe6-5ead1d4498e1',0,100),('bokheim:discrete-math:combinatorics','unified:21b67f0c-588c-5f8a-bfe6-5ead1d4498e1',0,200),('bokheim:discrete-math:coding','unified:21b67f0c-588c-5f8a-bfe6-5ead1d4498e1',0,300);
UPDATE source_selector SET concept_id='bokheim:discrete-math:graph-theory' WHERE concept_id='unified:21b67f0c-588c-5f8a-bfe6-5ead1d4498e1' AND system_id='lcc' AND selector LIKE 'QA166%';
UPDATE source_selector SET concept_id='bokheim:discrete-math:combinatorics' WHERE concept_id='unified:21b67f0c-588c-5f8a-bfe6-5ead1d4498e1' AND system_id='lcc' AND selector LIKE 'QA267%';
UPDATE source_selector SET concept_id='bokheim:discrete-math:coding' WHERE concept_id='unified:21b67f0c-588c-5f8a-bfe6-5ead1d4498e1' AND system_id='lcc' AND selector LIKE 'QA268%';
UPDATE source_selector SET concept_id='bokheim:discrete-math:combinatorics' WHERE concept_id='unified:21b67f0c-588c-5f8a-bfe6-5ead1d4498e1';

-- Replacement for 98 (Cosmogony/Cosmology): Number Theory divides by mathematical topic.
INSERT INTO concept VALUES ('bokheim:number-theory:general','Number Theory—General'),('bokheim:number-theory:divisibility','Divisibility, Congruences & Diophantine Analysis'),('bokheim:number-theory:forms','Number-Theoretic Forms'),('bokheim:number-theory:higher-congruences','Higher Congruences, Residues & Fermat Theory'),('bokheim:number-theory:primes','Prime Distribution & Numerical Functions'),('bokheim:number-theory:algebraic','Algebraic Numbers & Fields');
INSERT INTO concept_parent VALUES ('bokheim:number-theory:general','unified:bbb83323-48af-58e9-8990-cfb664cfe214',0,100),('bokheim:number-theory:divisibility','unified:bbb83323-48af-58e9-8990-cfb664cfe214',0,200),('bokheim:number-theory:forms','unified:bbb83323-48af-58e9-8990-cfb664cfe214',0,300),('bokheim:number-theory:higher-congruences','unified:bbb83323-48af-58e9-8990-cfb664cfe214',0,400),('bokheim:number-theory:primes','unified:bbb83323-48af-58e9-8990-cfb664cfe214',0,500),('bokheim:number-theory:algebraic','unified:bbb83323-48af-58e9-8990-cfb664cfe214',0,600);
UPDATE source_selector SET concept_id='bokheim:number-theory:general' WHERE concept_id='unified:bbb83323-48af-58e9-8990-cfb664cfe214' AND system_id='lcc' AND selector LIKE 'QA241%';
UPDATE source_selector SET concept_id='bokheim:number-theory:divisibility' WHERE concept_id='unified:bbb83323-48af-58e9-8990-cfb664cfe214' AND system_id='lcc' AND selector LIKE 'QA242%';
UPDATE source_selector SET concept_id='bokheim:number-theory:forms' WHERE concept_id='unified:bbb83323-48af-58e9-8990-cfb664cfe214' AND system_id='lcc' AND selector LIKE 'QA243%';
UPDATE source_selector SET concept_id='bokheim:number-theory:higher-congruences' WHERE concept_id='unified:bbb83323-48af-58e9-8990-cfb664cfe214' AND system_id='lcc' AND (selector LIKE 'QA244%' OR selector LIKE 'QA245%');
UPDATE source_selector SET concept_id='bokheim:number-theory:primes' WHERE concept_id='unified:bbb83323-48af-58e9-8990-cfb664cfe214' AND system_id='lcc' AND selector LIKE 'QA246%';
UPDATE source_selector SET concept_id='bokheim:number-theory:algebraic' WHERE concept_id='unified:bbb83323-48af-58e9-8990-cfb664cfe214' AND system_id='lcc' AND selector LIKE 'QA247%';
UPDATE source_selector SET concept_id='bokheim:number-theory:general' WHERE concept_id='unified:bbb83323-48af-58e9-8990-cfb664cfe214';

COMMIT;
