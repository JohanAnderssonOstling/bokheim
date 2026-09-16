INSERT INTO curated.unified_concept_route(path,concept_id,parent_path,label) VALUES
             ('Fiction',900001,'','Fiction'),('Fiction / Crime',900002,'Fiction','Crime'),('Nature',900003,'','Nature');
         INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'alpha.epub', 'a'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'beta.epub', 'b');
         -- Crime on both books, Nature on one: the chips are ranked by how much
         -- of the author's shelf carries them, not by the taxonomy's order.
         INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version)
             SELECT row_id,900002,1 FROM book;
         INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version)
             SELECT row_id,900003,1 FROM book WHERE content_hash=printf('%064x',1);