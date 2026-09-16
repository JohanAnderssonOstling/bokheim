INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT '00000000-0000-0000-0000-000000000000',row_id,'Audio.m4b','fixture',1 FROM book WHERE content_hash=?1
