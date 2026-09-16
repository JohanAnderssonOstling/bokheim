-- name: get_recently_read_cards?
-- param: limit: i32
-- param: downloaded_only: i32
-- param: finished_threshold: f64
-- The limit applies to books in progress and to finished books separately.
-- A single LIMIT over the whole list let a run of finished books crowd out
-- the ones still being read, which is the list Home exists to show; ranking
-- within each group spends the limit twice in one statement rather than
-- asking for the two lists in two round trips.
WITH read_cards AS (
  SELECT card.content_hash, card.title, card.author, card.description, card.progress,
         card.downloaded, card.download_requested, card.format_category,
         card.audiobook_duration_ms, card.audiobook_chapter_count,
         COALESCE(card.progress, 0) >= :finished_threshold AS finished,
         reading_version.changed_at AS changed_at, card.subtitle
  FROM book_cards card
  JOIN sync_state_version reading_version
    ON reading_version.state_kind = 'reading_position'
   AND reading_version.state_key = card.content_hash
  WHERE (:downloaded_only = 0 OR card.downloaded)
)
SELECT content_hash, title, author, description, progress,
       downloaded, download_requested, format_category,
       audiobook_duration_ms, audiobook_chapter_count, finished, subtitle
FROM (
    SELECT *, ROW_NUMBER() OVER (
        PARTITION BY finished
        ORDER BY changed_at DESC, normalize_search_text(COALESCE(title, '')), title, content_hash
    ) AS position
    FROM read_cards
)
WHERE position <= :limit
ORDER BY finished, position;

-- name: get_most_progress_cards?
-- param: limit: i32
-- param: downloaded_only: i32
-- param: finished_threshold: f64
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count, card.subtitle
FROM book_cards card
WHERE (:downloaded_only = 0 OR card.downloaded)
  AND COALESCE(card.progress, 0) > 0
  AND COALESCE(card.progress, 0) < :finished_threshold
ORDER BY card.progress DESC,
         normalize_search_text(COALESCE(card.title, '')), card.title, card.content_hash
LIMIT :limit;

-- name: get_recently_added_cards?
-- param: limit: i32
-- param: downloaded_only: i32
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.format_category,
       card.audiobook_duration_ms, card.audiobook_chapter_count, card.subtitle
FROM book_cards card
JOIN book ON book.row_id = card.book_row_id
WHERE (:downloaded_only = 0 OR card.downloaded)
ORDER BY COALESCE(book.added_at, 0) DESC, LOWER(COALESCE(card.title, '')), card.content_hash
LIMIT :limit;

-- name: count_all_books?
-- Counts only books with a live placement, matching what the carousels display.
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL
  AND hidden_at IS NULL
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_row_id = book.row_id AND bd.deleted_at IS NULL);

-- name: count_books_with_added_at?
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL AND added_at IS NOT NULL AND added_at > 0
  AND hidden_at IS NULL
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_row_id = book.row_id AND bd.deleted_at IS NULL);

