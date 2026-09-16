INSERT INTO curated.unified_concept_route VALUES
            ('World',909999,'','World'),
            ('World / U.S.',910000,'World','U.S.'),
            ('World / U.S. / A',910001,'World / U.S.','Branch A'),
            ('World / U.S. / A / Leaf',910002,'World / U.S. / A','A leaf'),
            ('World / U.S. / B',910003,'World / U.S.','Branch B'),
            ('World / U.S. / B / Leaf',910004,'World / U.S. / B','B leaf'),
            ('World / U.S. / C',910005,'World / U.S.','Branch C'),
            ('World / U.S. / C / Leaf',910006,'World / U.S. / C','C leaf'),
            ('World / U.S. / Empty',910007,'World / U.S.','Empty');
         INSERT INTO book(content_hash,title,format) VALUES
            (printf('%064x',901),'Alpha book','epub'),
            (printf('%064x',902),'Beta book','pdf'),
            (printf('%064x',903),'Gamma book','m4b');
         INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded)
            SELECT '00000000-0000-0000-0000-000000000000',row_id,title,title,title='Alpha book' FROM book;
         INSERT INTO book_language(book_row_id,position,language_tag)
            SELECT row_id,0,CASE WHEN title='Beta book' THEN 'sv-SE' ELSE 'en-US' END FROM book;
         INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version)
            SELECT row_id,CASE title WHEN 'Alpha book' THEN 910002 WHEN 'Beta book' THEN 910004 ELSE 910006 END,1 FROM book;