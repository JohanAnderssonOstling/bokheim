INSERT OR IGNORE INTO subject_browse_dirty_book SELECT row_id FROM book;
            DELETE FROM library_subject; DELETE FROM subject_browse_member;
            DELETE FROM subject_browse_child; DELETE FROM subject_browse_book; DELETE FROM subject_browse_count;
            DELETE FROM subject_browse_raw_child; DELETE FROM subject_browse_raw_book;
            DELETE FROM subject_book_ownership;