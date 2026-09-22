INSERT INTO book_contributor(book_row_id, author_identity_id, position, name, role)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash), :author_identity_id, :position, :name, :role)
ON CONFLICT(book_row_id, position) DO UPDATE SET
    author_identity_id=excluded.author_identity_id,
    name=excluded.name,
    role=excluded.role
WHERE book_contributor.author_identity_id IS NOT excluded.author_identity_id
   OR book_contributor.name IS NOT excluded.name
   OR book_contributor.role IS NOT excluded.role;
