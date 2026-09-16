INSERT INTO curated.unified_concept_route(path,concept_id,parent_path,label) VALUES
            ('Facet',900001,'','Facet'),('Facet / Child',900000,'Facet','Child');
         INSERT INTO book(content_hash,title,format) VALUES
            (printf('%064x',201),'German PDF','pdf'),(printf('%064x',202),'German EPUB','epub'),(printf('%064x',203),'English PDF','pdf');
         INSERT INTO book_language(book_row_id,position,language_tag)
            SELECT row_id,0,CASE WHEN title LIKE 'German%' THEN 'de' ELSE 'en' END FROM book WHERE row_id IN (SELECT row_id FROM book WHERE title IN ('German PDF','German EPUB','English PDF'));
         INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash)
            SELECT '00000000-0000-0000-0000-000000000000',row_id,title,printf('facet-%d',row_id) FROM book WHERE title IN ('German PDF','German EPUB','English PDF');
         INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version)
            SELECT row_id,900000,1 FROM book WHERE title IN ('German PDF','German EPUB','English PDF');