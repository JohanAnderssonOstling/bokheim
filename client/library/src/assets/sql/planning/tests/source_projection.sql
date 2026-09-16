INSERT INTO dir(id,parent_id,name) VALUES
('30000000-0000-0000-0000-000000000000','00000000-0000-0000-0000-000000000000','Old source');
INSERT INTO local_file_projection(content_hash,dir_id,relative_path)
SELECT content_hash,'30000000-0000-0000-0000-000000000000','Old source/' || row_id || '.epub' FROM book;
DELETE FROM local_directory_work;
