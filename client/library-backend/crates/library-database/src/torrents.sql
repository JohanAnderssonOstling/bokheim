-- Torrent transfers and book associations are device-local, never sync registers.
CREATE TABLE IF NOT EXISTS torrent (
    info_hash TEXT PRIMARY KEY NOT NULL,
    metadata BLOB NOT NULL,
    source TEXT,
    staging_path TEXT NOT NULL,
    output_path TEXT NOT NULL,
    imported INTEGER NOT NULL DEFAULT 0 CHECK(imported IN (0,1)),
    publishing INTEGER NOT NULL DEFAULT 0 CHECK(publishing IN (0,1)),
    keep_seeding INTEGER NOT NULL DEFAULT 1 CHECK(keep_seeding IN (0,1)),
    folder_id TEXT REFERENCES dir(id) ON DELETE SET NULL,
    root_book_row_id INTEGER REFERENCES book(row_id) ON DELETE SET NULL,
    error TEXT
) STRICT;
CREATE TABLE IF NOT EXISTS torrent_file (
    info_hash TEXT NOT NULL REFERENCES torrent(info_hash) ON DELETE CASCADE,
    file_index INTEGER NOT NULL CHECK(file_index >= 0),
    relative_name TEXT NOT NULL,
    PRIMARY KEY(info_hash,file_index)
) STRICT;
CREATE TABLE IF NOT EXISTS book_torrent (
    book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    torrent_info_hash TEXT NOT NULL,
    torrent_file_index INTEGER NOT NULL,
    PRIMARY KEY(book_row_id,torrent_info_hash,torrent_file_index),
    UNIQUE(torrent_info_hash,torrent_file_index),
    FOREIGN KEY(torrent_info_hash,torrent_file_index)
        REFERENCES torrent_file(info_hash,file_index) ON DELETE CASCADE
) STRICT;
