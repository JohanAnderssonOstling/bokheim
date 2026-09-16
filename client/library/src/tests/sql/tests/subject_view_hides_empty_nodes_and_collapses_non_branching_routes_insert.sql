INSERT INTO curated.unified_concept_route(path,concept_id,parent_path,label) VALUES
            ('Test',900011,'','Test'),
            ('Test / United States',900015,'Test','United States'),
            ('Test / United States / Texas',900014,'Test / United States','Texas'),
            ('Test / United States / Texas / Austin',900007,'Test / United States / Texas','Austin'),
            ('Test / United States / Texas / Dallas',900008,'Test / United States / Texas','Dallas'),
            ('Test / Solo',900012,'Test','Solo'),
            ('Test / Solo / Solo Leaf',900013,'Test / Solo','Solo Leaf'),
            ('Test / Empty',900009,'Test','Empty');
         INSERT INTO book(content_hash,title,format) VALUES
            (printf('%064x',101),'Austin book','epub'),
            (printf('%064x',102),'Dallas book','epub'),
            (printf('%064x',103),'Solo book','epub');
         INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash) VALUES
            ('00000000-0000-0000-0000-000000000000',(SELECT row_id FROM book WHERE content_hash=printf('%064x',101)),'austin.epub','austin'),
            ('00000000-0000-0000-0000-000000000000',(SELECT row_id FROM book WHERE content_hash=printf('%064x',102)),'dallas.epub','dallas'),
            ('00000000-0000-0000-0000-000000000000',(SELECT row_id FROM book WHERE content_hash=printf('%064x',103)),'solo.epub','solo');
         INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version) VALUES
            ((SELECT row_id FROM book WHERE content_hash=printf('%064x',101)),900007,1),
            ((SELECT row_id FROM book WHERE content_hash=printf('%064x',102)),900008,1),
            ((SELECT row_id FROM book WHERE content_hash=printf('%064x',103)),900013,1);