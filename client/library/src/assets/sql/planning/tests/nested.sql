INSERT INTO dir(id,parent_id,name) VALUES
('10000000-0000-0000-0000-000000000000','00000000-0000-0000-0000-000000000000','Shelf'),
('20000000-0000-0000-0000-000000000000','10000000-0000-0000-0000-000000000000','Nested');
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded)
SELECT '20000000-0000-0000-0000-000000000000',row_id,row_id || '.epub','fp',0 FROM book;
UPDATE book_dir SET deleted_at=1 WHERE dir_id='00000000-0000-0000-0000-000000000000';
DELETE FROM local_book_work;
DELETE FROM local_file_work;
DELETE FROM local_directory_work;
