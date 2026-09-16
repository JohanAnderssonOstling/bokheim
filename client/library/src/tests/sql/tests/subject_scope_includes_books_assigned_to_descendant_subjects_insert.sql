INSERT INTO curated.unified_concept_route(path,concept_id,parent_path,label) VALUES
            ('Fixture',900005,'','Fixture'),
            ('Alternate',900002,'','Alternate'),
            ('Fixture / History',900004,'Fixture','History'),
            ('Alternate / History',900004,'Alternate','History'),
            ('Fixture / History / Europe',900003,'Fixture / History','Europe'),
            ('Fixture / Science',900006,'Fixture','Science');
         INSERT INTO book(content_hash, title, format) VALUES
            (printf('%064x',1), 'Parent history', 'epub'),
            (printf('%064x',2), 'Child history', 'epub'),
            (printf('%064x',3), 'Science', 'epub');
         INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
            ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'parent.epub', 'parent'),
            ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'child.epub', 'child'),
            ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'science.epub', 'science');
         INSERT INTO book_unified_concept(book_row_id, concept_id, mapper_version) VALUES
            ((SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 900004, 4),
            ((SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 900003, 4),
            ((SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 900006, 4);