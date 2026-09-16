INSERT INTO dir(id,parent_id,name) VALUES
        ('00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-000000000000','Humanities'),
        ('00000000-0000-0000-0000-000000000002','00000000-0000-0000-0000-000000000001','Religion'),
        ('00000000-0000-0000-0000-000000000003','00000000-0000-0000-0000-000000000002','Buddhism'),
        ('00000000-0000-0000-0000-000000000004','00000000-0000-0000-0000-000000000000','Mixed'),
        ('00000000-0000-0000-0000-000000000005','00000000-0000-0000-0000-000000000004','Child'),
        ('00000000-0000-0000-0000-000000000006','00000000-0000-0000-0000-000000000000','Empty'),
        ('00000000-0000-0000-0000-000000000007','00000000-0000-0000-0000-000000000001','.hidden');
        INSERT INTO book(content_hash,title,format) VALUES
        (printf('%064x',8101),'Religion book','epub'),
        (printf('%064x',8102),'Direct book','pdf'),
        (printf('%064x',8103),'Child book','epub');
        INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded)
        SELECT CASE title WHEN 'Religion book' THEN '00000000-0000-0000-0000-000000000003'
            WHEN 'Direct book' THEN '00000000-0000-0000-0000-000000000004'
            ELSE '00000000-0000-0000-0000-000000000005' END,row_id,title,title,title='Religion book' FROM book;