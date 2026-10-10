INSERT INTO library_blob_claim(library_id,user_id,content_hash) VALUES($1,$2,$3) ON CONFLICT(library_id,content_hash) DO NOTHING;
