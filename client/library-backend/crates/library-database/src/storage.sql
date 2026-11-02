CREATE TABLE IF NOT EXISTS library_storage (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    configuration TEXT NOT NULL
) STRICT;
