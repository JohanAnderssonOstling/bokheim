CREATE TABLE IF NOT EXISTS isbn_classification(
    isbn13 INTEGER NOT NULL,
    scheme INTEGER NOT NULL CHECK(scheme IN (1,2)),
    notation TEXT NOT NULL,
    PRIMARY KEY(isbn13,scheme,notation)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS isbn_classification_by_code
    ON isbn_classification(scheme,notation,isbn13);

CREATE TABLE IF NOT EXISTS title_author_classification(
    normalized_title TEXT NOT NULL,
    normalized_subtitle TEXT NOT NULL,
    normalized_author TEXT NOT NULL,
    publication_year INTEGER NOT NULL,
    normalized_publisher TEXT NOT NULL,
    scheme INTEGER NOT NULL CHECK(scheme IN (1,2)),
    notation TEXT NOT NULL,
    PRIMARY KEY(normalized_title,normalized_subtitle,normalized_author,publication_year,normalized_publisher,scheme,notation)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS title_author_classification_lookup
    ON title_author_classification(normalized_title,normalized_subtitle,normalized_author,publication_year,normalized_publisher);

-- Older active snapshots may predate the optional subtitle table.
CREATE TABLE IF NOT EXISTS edition_subtitle(
    edition_id INTEGER PRIMARY KEY,
    subtitle TEXT NOT NULL,
    normalized_title TEXT NOT NULL
);

INSERT OR IGNORE INTO isbn_classification(isbn13,scheme,notation)
SELECT DISTINCT edition_isbn.isbn13,edition_classification.scheme,edition_classification.notation
FROM edition_isbn
JOIN edition_classification USING(edition_id);

INSERT OR IGNORE INTO title_author_classification(
    normalized_title,normalized_subtitle,normalized_author,publication_year,normalized_publisher,scheme,notation
)
SELECT DISTINCT
    bibliography.normalized_title,
    lower(trim(COALESCE(subtitle.subtitle,''))),
    lower(trim(author.name)),
    COALESCE(bibliography.publication_year,0),
    publisher.normalized_name,
    classification.scheme,
    classification.notation
FROM edition_classification classification
JOIN edition_bibliography bibliography USING(edition_id)
JOIN edition_author edition_author USING(edition_id)
JOIN author ON author.author_id=edition_author.author_id
LEFT JOIN edition_subtitle subtitle USING(edition_id)
LEFT JOIN edition_publisher publisher ON publisher.edition_id=classification.edition_id AND publisher.position=1
WHERE trim(author.name)!='';

ANALYZE isbn_classification;
ANALYZE title_author_classification;
