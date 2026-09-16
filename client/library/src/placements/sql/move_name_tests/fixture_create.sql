CREATE TABLE dir(id TEXT PRIMARY KEY,parent_id TEXT,name TEXT,name_key TEXT GENERATED ALWAYS AS(portable_name_key(name)) STORED,deleted_at INTEGER);
        CREATE UNIQUE INDEX idx_dir_live_path ON dir(parent_id,name_key) WHERE deleted_at IS NULL AND id!='00000000-0000-0000-0000-000000000000';
        CREATE TABLE book_dir(dir_id TEXT,book_row_id INTEGER,file_name TEXT,deleted_at INTEGER,PRIMARY KEY(dir_id,book_row_id));
        CREATE UNIQUE INDEX idx_book_dir_live_path ON book_dir(dir_id,file_name) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_book_dir_live_portable_name ON book_dir(dir_id,portable_name_key(file_name)) WHERE deleted_at IS NULL;
