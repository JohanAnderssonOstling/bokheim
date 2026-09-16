INSERT INTO book_contributor(book_row_id, position, author_identity_id, name, sort_name, role)
         SELECT book_contributor.book_row_id, 7, book_contributor.author_identity_id, 'J.R.R. Tolkien', 'Tolkien, J.R.R.', book_contributor.role
         FROM book_contributor LIMIT 1