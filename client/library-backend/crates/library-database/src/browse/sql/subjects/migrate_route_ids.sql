-- Only derived navigation state is discarded. Assignments and sync state survive.
DROP TABLE library_subject;
DROP TABLE subject_browse_member;
DROP TABLE subject_book_ownership;
DROP TABLE subject_browse_child;
DROP TABLE subject_browse_book;
DROP TABLE subject_browse_count;
DROP TABLE subject_browse_raw_child;
DROP TABLE subject_browse_raw_book;
CREATE TABLE library_subject(route_id INTEGER PRIMARY KEY);
CREATE TABLE subject_browse_member(
    route_id INTEGER NOT NULL, book_row_id INTEGER NOT NULL, direct INTEGER NOT NULL,
    PRIMARY KEY(route_id,book_row_id)
) WITHOUT ROWID;
CREATE INDEX subject_browse_member_book ON subject_browse_member(book_row_id,route_id);
CREATE TABLE subject_book_ownership(
    route_id INTEGER NOT NULL,
    book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    assignment_count INTEGER NOT NULL CHECK(assignment_count>0),
    PRIMARY KEY(route_id,book_row_id)
);
CREATE INDEX idx_subject_book_ownership_book ON subject_book_ownership(book_row_id,route_id);
ALTER TABLE subject_browse_state ADD COLUMN taxonomy_revision TEXT NOT NULL DEFAULT '';
UPDATE subject_browse_state SET built_revision=-1;
