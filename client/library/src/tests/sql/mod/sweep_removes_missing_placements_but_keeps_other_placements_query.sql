INSERT INTO dir(id,parent_id,name) VALUES ('64310000-0000-0000-0000-0000881d13e6','00000000-0000-0000-0000-000000000000','D1'),('64320000-0000-0000-0000-0000871d1253','00000000-0000-0000-0000-000000000000','D2');
         INSERT INTO book(content_hash,format) VALUES
                 (printf('%064x',1),'epub'),(printf('%064x',2),'epub'),(printf('%064x',3),'epub'),(printf('%064x',4),'epub'),(printf('%064x',5),'epub');
         INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, is_downloaded, last_scan) VALUES
                 ('64310000-0000-0000-0000-0000881d13e6',(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)),'keep.epub','fp1',1,10),
                 ('64310000-0000-0000-0000-0000881d13e6',(SELECT row_id FROM book WHERE content_hash=printf('%064x',4)),'blip.epub','fp4',1,9),
                 ('64310000-0000-0000-0000-0000881d13e6',(SELECT row_id FROM book WHERE content_hash=printf('%064x',2)),'gone.epub','fp2',1,8),
                 ('64320000-0000-0000-0000-0000871d1253',(SELECT row_id FROM book WHERE content_hash=printf('%064x',2)),'moved.epub','fp9',1,10),
                 ('64320000-0000-0000-0000-0000871d1253',(SELECT row_id FROM book WHERE content_hash=printf('%064x',3)),'remote.epub',printf('%064x',3),0,0),
                 ('64320000-0000-0000-0000-0000871d1253',(SELECT row_id FROM book WHERE content_hash=printf('%064x',5)),'fresh.epub',printf('%064x',5),1,0);