-- Ensure the book exists before reading its facts.
INSERT INTO book(content_hash)
VALUES(:content_hash)
ON CONFLICT(content_hash) DO NOTHING;
