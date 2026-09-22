-- Social Work already has ranged leaves for its meaningful subdivisions.
-- Retain its exact printed umbrella and general-work headings, which are valid
-- on a nonleaf, but remove the permissive range that overlaps every child.

DELETE FROM lcc_range
WHERE concept_id=5467
  AND start_code='HV40'
  AND end_code='HV69.2';
