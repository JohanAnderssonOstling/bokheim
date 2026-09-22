-- Curated invariant: genuine permissive LCC ranges belong only to leaves.
-- Non-leaf concepts may retain exact selectors.
DELETE FROM source_selector AS s
WHERE s.system_id='lcc'
  AND s.selector LIKE '%..%@%'
  AND EXISTS (
    SELECT 1 FROM concept_parent cp WHERE cp.parent_concept_id=s.concept_id
  )
  AND substr(s.selector,1,instr(s.selector,'..')-1)
      <> substr(
        substr(s.selector,instr(s.selector,'..')+2),
        1,
        instr(substr(s.selector,instr(s.selector,'..')+2),'@')-1
      );
