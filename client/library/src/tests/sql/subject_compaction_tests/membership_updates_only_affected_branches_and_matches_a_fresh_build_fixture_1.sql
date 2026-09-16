INSERT INTO book(content_hash,title,format) VALUES(printf('%064x',904),'Delta book','epub');
         INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT '00000000-0000-0000-0000-000000000000',row_id,'delta.epub','delta',0 FROM book WHERE title='Delta book';
         INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version) SELECT row_id,910002,1 FROM book WHERE title='Delta book';
