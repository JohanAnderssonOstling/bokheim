-- Rebuildable search projection. Canonical annotation records remain in sync state.
CREATE TABLE IF NOT EXISTS notebook_annotation (
    id TEXT PRIMARY KEY NOT NULL,
    book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    body BLOB NOT NULL,
    search_text TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    toc_ordinal INTEGER,
    progress REAL,
    external INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS notebook_annotation_book ON notebook_annotation(book_row_id, created_at DESC, id);
CREATE INDEX IF NOT EXISTS notebook_annotation_location ON notebook_annotation(book_row_id, toc_ordinal, progress, id);
CREATE TRIGGER IF NOT EXISTS notebook_annotation_removed AFTER DELETE ON annotation
BEGIN DELETE FROM notebook_annotation WHERE id=OLD.id AND external=0; END;

-- Only annotation changes expire the Notebook selection; cover and job writes do not.
CREATE TABLE IF NOT EXISTS notebook_revision (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    revision INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT OR IGNORE INTO notebook_revision(singleton) VALUES(1);
CREATE TRIGGER IF NOT EXISTS notebook_revision_insert AFTER INSERT ON notebook_annotation
BEGIN UPDATE notebook_revision SET revision=revision+1 WHERE singleton=1; END;
CREATE TRIGGER IF NOT EXISTS notebook_revision_delete AFTER DELETE ON notebook_annotation
BEGIN UPDATE notebook_revision SET revision=revision+1 WHERE singleton=1; END;
CREATE TRIGGER IF NOT EXISTS notebook_revision_update AFTER UPDATE ON notebook_annotation
WHEN OLD.body IS NOT NEW.body OR OLD.book_row_id IS NOT NEW.book_row_id
    OR OLD.search_text IS NOT NEW.search_text OR OLD.created_at IS NOT NEW.created_at
    OR OLD.toc_ordinal IS NOT NEW.toc_ordinal OR OLD.progress IS NOT NEW.progress OR OLD.external IS NOT NEW.external
BEGIN UPDATE notebook_revision SET revision=revision+1 WHERE singleton=1; END;
