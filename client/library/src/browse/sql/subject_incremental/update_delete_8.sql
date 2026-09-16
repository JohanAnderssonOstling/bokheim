DELETE FROM subject_browse_dirty_book;
        UPDATE subject_browse_state SET built_revision=tree_revision WHERE id=1;
        UPDATE browse_projection_state SET dirty=0 WHERE projection='subject_ownership';