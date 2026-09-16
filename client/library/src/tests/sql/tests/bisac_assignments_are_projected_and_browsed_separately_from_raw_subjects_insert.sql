INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES
         ('00000000-0000-0000-0000-000000000000',(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)),'one.epub','one',1),
         ('00000000-0000-0000-0000-000000000000',(SELECT row_id FROM book WHERE content_hash=printf('%064x',2)),'two.epub','two',1),
         ('00000000-0000-0000-0000-000000000000',(SELECT row_id FROM book WHERE content_hash=printf('%064x',3)),'three.epub','three',1);