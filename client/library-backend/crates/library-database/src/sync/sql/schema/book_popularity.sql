ALTER TABLE book ADD COLUMN popularity_rating_count REAL CHECK(popularity_rating_count >= 0);
