-- This table is a local projection; synchronized book records retain their text.
ALTER TABLE book_description RENAME TO book_description_plain;
CREATE TABLE book_description(
    book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE,
    description ANY NOT NULL CHECK(typeof(description) IN('text','blob'))
) STRICT;
INSERT INTO book_description(book_row_id,description)
SELECT book_row_id,pack_description(description) FROM book_description_plain;
DROP TABLE book_description_plain;
