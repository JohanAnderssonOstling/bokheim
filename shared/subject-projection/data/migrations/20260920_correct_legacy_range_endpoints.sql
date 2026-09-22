-- Reviewed against the Library of Congress D–DR 2025 schedule, pp. 24, 447:
-- https://www.loc.gov/aba/publications/FreeLCC/LCC_D-DR2025TEXT.pdf
-- D552.A-Z is a printed interval: the upper boundary is D552.Z.
-- DK908.68.2 is malformed. The current schedule ends these numbered entries
-- at DK908.68. Keep the separate historical DK908.682 exact selector.
UPDATE lcc_range SET end_code='D552.Z'
WHERE concept_id=10305 AND start_code='D551' AND end_code='D552.A-Z';
UPDATE lcc_range SET end_code='DK908.68'
WHERE concept_id=54459 AND start_code='DK908.67' AND end_code='DK908.68.2';
