WITH visible_book_languages AS (
             SELECT DISTINCT
                 language.book_row_id,
                 CASE
                     WHEN instr(replace(lower(trim(language.language_tag)), '_', '-'), '-') > 0
                         THEN substr(
                             replace(lower(trim(language.language_tag)), '_', '-'),
                             1,
                             instr(replace(lower(trim(language.language_tag)), '_', '-'), '-') - 1
                         )
                     ELSE replace(lower(trim(language.language_tag)), '_', '-')
                 END AS base_language
             FROM book_language AS language
             JOIN book ON book.row_id=language.book_row_id
             WHERE book.deleted_at IS NULL
               AND book.hidden_at IS NULL
               AND trim(language.language_tag) != ''
               AND EXISTS (
                   SELECT 1
                   FROM book_dir AS placement
                   WHERE placement.book_row_id=book.row_id
                     AND placement.deleted_at IS NULL
               )
         )
         SELECT base_language, COUNT(*)
         FROM visible_book_languages
         WHERE base_language != ''
         GROUP BY base_language
         ORDER BY base_language