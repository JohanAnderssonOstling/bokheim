SELECT tree_revision,built_revision,
    EXISTS(SELECT 1 FROM subject_browse_member)
    AND NOT EXISTS(SELECT 1 FROM subject_browse_member WHERE path IN('',?1))
FROM subject_browse_state WHERE id=1
