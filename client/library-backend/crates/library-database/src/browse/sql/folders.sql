-- name: folder_children?
-- param: directory_ids: &str
SELECT child.parent_id,child.id,child.name
FROM json_each(:directory_ids) selected
JOIN dir parent ON parent.id=selected.value AND parent.deleted_at IS NULL
JOIN dir child ON child.parent_id=parent.id AND child.id!=parent.id AND child.deleted_at IS NULL;

-- name: folder_placements?
-- param: directory_ids: &str
-- A book placed in two of the folders is listed once per placement.
SELECT placement.book_row_id,placement.dir_id,placement.is_downloaded
FROM json_each(:directory_ids) selected
JOIN dir directory ON directory.id=selected.value AND directory.deleted_at IS NULL
JOIN book_dir placement ON placement.dir_id=directory.id AND placement.deleted_at IS NULL
JOIN book ON book.row_id=placement.book_row_id AND book.deleted_at IS NULL;

-- name: folder_direct_counts?
-- param: directory_ids: &str
-- param: file_types: i32
-- param: languages: &str
-- param: hide_finished: i32
-- param: finished_threshold: f64
-- Each row counts only one directory's live placements, including duplicate books.
SELECT selected.value,count(book.row_id),
       count(CASE WHEN placement.is_downloaded=1 THEN book.row_id END)
FROM json_each(:directory_ids) selected
LEFT JOIN book_dir placement ON placement.dir_id=selected.value AND placement.deleted_at IS NULL
LEFT JOIN book ON book.row_id=placement.book_row_id
  AND book.deleted_at IS NULL
  AND (:hide_finished=0 OR COALESCE(book.read_progress,0)<:finished_threshold)
  AND (:file_types=0 OR (:file_types & book.format)!=0)
  AND (:languages='' OR EXISTS(SELECT 1 FROM book_language language
      WHERE language.book_row_id=book.row_id AND instr(:languages,',' || language.language || ',')>0))
GROUP BY selected.value;
