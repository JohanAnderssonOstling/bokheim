-- name: author_books?
-- param: author_role: Vec<u8>
-- Every author credit as (author, display name, book) rows. An author files
-- under the smallest spelling credited to them; a book credited twice to one
-- identity — two spellings that resolved together — is still one book.
SELECT author_identity.stable_id AS author_id,
       MIN(credit.name) OVER (PARTITION BY author_identity.id) AS display_name,
       credit.book_row_id
FROM author_identity
JOIN book_contributor credit ON credit.author_identity_id = author_identity.id AND credit.role = :author_role
WHERE TRIM(credit.name) != '';
