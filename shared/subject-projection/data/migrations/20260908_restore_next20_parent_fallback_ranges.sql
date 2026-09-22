-- Preserve complete coverage for Cutter-bearing and unforeseen calls. Narrower
-- child ranges remain installed and win by specificity; these original parent
-- ranges serve only as the general fallback rather than fragile decimal gaps.
BEGIN;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HG201','HG230.29',12837),('HG230.31','HG230.49',12837),
 ('HG230.71','HG257.99',12837),
 ('NA1','NA9428',193),
 ('TL1','TL4050',404),('TL1','TL483',404),
 ('SF421','SF440.2.Z',12460),
 ('GN','GN',5056),('GN1','GN890',5056),
 ('RC512','RC528',2715),('RC512','RC569.5',2715),
 ('RC530','RC552',2715),('RC554','RC569.5',2715),
 ('JN3201','JN4980',11604),
 ('QL750','QL795',5050),
 ('JN101','JN1371',11598),
 ('HB1','HB846.8',5593),
 ('GB3','GB5030',2421),
 ('TX901','TX946.5',3119),('TX907','TX907.5',3119),
 ('HQ767.8','HQ792.2',12281),
 ('JF','JS',5691),
 ('HF5410','HF5418.9',5189),('HF5801','HF6182.2',5189),
 ('QD1','QD999',4803),
 ('HD39','HD40.7',5183),('HG4501','HG6052.Z',5183),
 ('GT500','GT2370',676),
 ('JC571','JC607',12531),
 ('R856','R857',2726);
COMMIT;
