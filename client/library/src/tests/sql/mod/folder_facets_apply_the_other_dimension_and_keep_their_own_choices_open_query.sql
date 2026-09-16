INSERT INTO dir(id,parent_id,name) VALUES ('61000000-0000-0000-0000-0000e40c292c','00000000-0000-0000-0000-000000000000','A'),('62000000-0000-0000-0000-0000e70c2de5','00000000-0000-0000-0000-000000000000','B');
         WITH RECURSIVE numbers(value) AS (SELECT 1 UNION ALL SELECT value+1 FROM numbers WHERE value<20)
         INSERT INTO book(content_hash,title,format)
         SELECT printf('%064x',value),'Book ' || value,CASE WHEN value<=10 OR value=20 THEN 'pdf' ELSE 'epub' END FROM numbers;
         INSERT INTO book_language(book_row_id,position,language_tag)
         SELECT row_id,0,CASE WHEN row_id=1 OR row_id>=11 THEN 'de' ELSE 'en' END FROM book;
         INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash)
         SELECT CASE WHEN row_id=20 THEN '62000000-0000-0000-0000-0000e70c2de5' ELSE '61000000-0000-0000-0000-0000e40c292c' END,row_id,'book-' || row_id,printf('local-%d',row_id) FROM book;