DELETE FROM subject_system WHERE id='lcc';
         INSERT INTO subject_system(id,version,matcher_version) VALUES('lcc','external',1);
         INSERT INTO subject_node(system_id,path,parent_path,name,code,assignable) VALUES
             ('lcc','LCC','','LCC',NULL,1),
             ('lcc','LCC / LCC','LCC','LCC',NULL,1),
             ('lcc','LCC / LCC / QA76.73.R3','LCC / LCC','QA76.73.R3',NULL,1);
         INSERT INTO book_subject_assignment(book_row_id,system_id,subject_path,matcher_version)
             VALUES((SELECT row_id FROM book WHERE content_hash=printf('%064x',1)),'lcc','LCC / LCC / QA76.73.R3',1);