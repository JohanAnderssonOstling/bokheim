SELECT card.content_hash,card.title,card.author,card.description,card.progress,
card.downloaded,card.download_requested,card.format_category,card.audiobook_duration_ms,card.audiobook_chapter_count,COALESCE(book.added_at,0),card.subtitle FROM subject_browse_book shown JOIN book_cards card ON card.book_row_id=shown.book_row_id
         JOIN book ON book.row_id=card.book_row_id WHERE shown.location=?1
    AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=card.book_row_id AND p.deleted_at IS NULL)
    AND (?2=0 OR card.downloaded)
    AND (?3=0 OR EXISTS(SELECT 1 FROM book_browse_format f WHERE f.book_row_id=card.book_row_id AND (f.format_category & ?3)!=0))
    AND (?4='' OR EXISTS(SELECT 1 FROM book_language l WHERE l.book_row_id=card.book_row_id AND
        (instr(?4,',' || lower(l.language_tag) || ',')>0 OR
         (instr(l.language_tag,'-')>0 AND instr(?4,',' || substr(lower(l.language_tag),1,instr(l.language_tag,'-')-1) || ',')>0))))
    ORDER BY CASE WHEN ?5=1 THEN (SELECT MAX(changed_at) FROM sync_state_version WHERE state_kind='reading_position' AND state_key=card.content_hash) END DESC,
    normalize_search_text(COALESCE(card.title,'')),card.title,card.content_hash