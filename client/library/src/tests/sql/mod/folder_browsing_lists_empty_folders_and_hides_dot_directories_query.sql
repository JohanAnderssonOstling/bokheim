INSERT INTO dir(id,parent_id,name) VALUES
        ('61000000-0000-0000-0000-0000e40c292c','00000000-0000-0000-0000-000000000000','Visible'),
        ('612d6368-696c-6400-0000-000093ae1f6b','61000000-0000-0000-0000-0000e40c292c','Nested'),
        ('62000000-0000-0000-0000-0000e70c2de5','00000000-0000-0000-0000-000000000000','Hidden only'),
        ('64310000-0000-0000-0000-0000881d13e6','62000000-0000-0000-0000-0000e70c2de5','.secret'),
        ('6f746865-7200-0000-0000-0000c87d8df5','00000000-0000-0000-0000-000000000000','.hidden'),
        ('64320000-0000-0000-0000-0000871d1253','6f746865-7200-0000-0000-0000c87d8df5','Searchable'),
        ('656d7074-7900-0000-0000-000018a7beee','61000000-0000-0000-0000-0000e40c292c','.cache'),
        ('656d7074-792d-6368-696c-6400598db709','00000000-0000-0000-0000-000000000000','Empty');
        INSERT INTO book(content_hash,title,format) VALUES
        (printf('%064x',1),'Visible book','epub'),
        (printf('%064x',2),'Hidden book','pdf');
        INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded)
        SELECT '612d6368-696c-6400-0000-000093ae1f6b',row_id,'visible.epub','local',1 FROM book WHERE title='Visible book';
        INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded)
        SELECT '61000000-0000-0000-0000-0000e40c292c',row_id,'duplicate.epub','local',1 FROM book WHERE title='Visible book';
        INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded)
        SELECT dir.id,book.row_id,'hidden.pdf','local',1 FROM dir CROSS JOIN book
        WHERE dir.id IN ('64310000-0000-0000-0000-0000881d13e6','64320000-0000-0000-0000-0000871d1253','656d7074-7900-0000-0000-000018a7beee') AND book.title='Hidden book';