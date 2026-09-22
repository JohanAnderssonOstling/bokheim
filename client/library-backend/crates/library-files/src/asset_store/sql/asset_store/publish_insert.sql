INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(?1,?2,?3) ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path
