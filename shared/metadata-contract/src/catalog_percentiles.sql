-- One retained source observation counts once, even if it binds several formats.
-- Rank within each original ranking source, including observations carried by
-- merged books. Unknowns are excluded; zero is observed. Average tied ranks span
-- 0–100; a singleton or an entirely tied cohort receives 50.
-- Reapplying this monotone projection to the same cohort preserves its scores.
WITH ranks AS MATERIALIZED (
 SELECT entry.source_id,entry.entry_id,
        rank() OVER (PARTITION BY entry.ranking_source_id ORDER BY entry.score) AS position,
        count(*) OVER (PARTITION BY entry.ranking_source_id,entry.score) AS ties,
        count(*) OVER (PARTITION BY entry.ranking_source_id) AS sample_size
 FROM source_entry entry JOIN source ranking ON ranking.id=entry.ranking_source_id
 WHERE ranking.name IN ('gutenberg.org','loyalbooks.com','standardebooks.org','unglue.it') AND entry.score IS NOT NULL
)
UPDATE source_entry AS entry
SET score=CASE WHEN ranks.sample_size=1 THEN 50.0
               ELSE 100.0*(ranks.position-1+(ranks.ties-1)/2.0)/(ranks.sample_size-1) END
FROM ranks WHERE ranks.source_id=entry.source_id AND ranks.entry_id=entry.entry_id;

-- Bilingual editions have an assigned neutral score, not an observed count.
UPDATE source_entry SET ranking_source_id=source_id,score=50.0
WHERE source_id IN (SELECT id FROM source WHERE name='bilinguator.com');
