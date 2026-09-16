INSERT OR IGNORE INTO book_external_entity(book_row_id,wikidata_id,kind,evidence_name,source)
                         VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5)