-- Remove unsupported exact dates from the 33 History period/event labels
-- identified by the LCC date-alignment audit.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH renames(concept_id, old_label, new_label) AS (VALUES
    (3411, 'Early 19th C., 1801/1809–1845', 'Early 19th Century'),
    (3553, '16th Century', '16th Century'),
    (8253, 'Ancien Régime, 1715–1789', 'Ancien Régime'),
    (9283, 'Napoleonic Era, 1796–1815', 'Napoleonic Era'),
    (10101, 'Bourbon Era, 1700–1808', 'Bourbon Era'),
    (10108, 'Contemporary Era, 2000–Present', 'Contemporary Era'),
    (10130, 'Early Modern Era, 1603–1707', 'Early Modern'),
    (10138, '1814–1907', '19th Century'),
    (10181, 'Enlightenment Era, 1740–1789', 'Enlightenment Era'),
    (10188, 'Roman Rule, 146 BC–323 AD', 'Roman Rule'),
    (10222, 'Unification, 1815–1871', 'Unification'),
    (10279, 'Brabant Revolution, 1789–1790', 'Brabant Revolution'),
    (10293, 'Medieval Low Countries, 843–1384', 'Medieval Low Countries'),
    (10296, 'Egypt, 1798–1879', 'Egypt'),
    (10372, 'Přemyslid Dynasty, c. 900–1310', 'Přemyslid Dynasty'),
    (10381, 'Late Ottoman Period, 1606–1699', 'Late Ottoman Rule'),
    (10639, '1830–1881', '19th Century'),
    (11457, 'Hundred Years’ War, 1337–1453', 'Hundred Years’ War'),
    (11464, 'Fourth Republic, 1946–1958', 'Fourth Republic'),
    (11512, 'Modern Era, 1808–1906', 'Modern Era'),
    (11536, 'Union Era, 1814–1905', 'Union Era'),
    (11553, 'Late Yugoslavia, 1980–1992', 'Late Yugoslavia'),
    (11564, 'Ottoman Reforms, 1789–1861', 'Ottoman Reforms'),
    (11578, 'House of Braganza, 1640–1816', 'House of Braganza'),
    (11579, 'Monarchy, 1816–1908', 'Monarchy'),
    (51008, '1901–1913', 'Early 20th Century'),
    (51013, 'The Twenties, 1919–1933', 'The Twenties'),
    (51017, 'F. D. Roosevelt, 1933–1945', 'F. D. Roosevelt'),
    (51018, 'Harry Truman, 1945–1953', 'Harry Truman'),
    (51082, 'George W. Bush, 2001–2009', 'George W. Bush'),
    (51083, 'Barack Obama, 2009–2017', 'Barack Obama'),
    (51084, 'Donald Trump, 2017–2021', 'Donald Trump'),
    (51085, 'Joe Biden, 2021–2025', 'Joe Biden')
)
INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
SELECT r.concept_id, r.old_label, 0
FROM renames r
JOIN concept c ON c.concept_id = r.concept_id
WHERE c.preferred_label = r.old_label;

UPDATE concept
SET preferred_label = CASE concept_id
    WHEN 3411 THEN 'Early 19th Century'
    WHEN 3553 THEN '16th Century'
    WHEN 8253 THEN 'Ancien Régime'
    WHEN 9283 THEN 'Napoleonic Era'
    WHEN 10101 THEN 'Bourbon Era'
    WHEN 10108 THEN 'Contemporary Era'
    WHEN 10130 THEN 'Early Modern'
    WHEN 10138 THEN '19th Century'
    WHEN 10181 THEN 'Enlightenment Era'
    WHEN 10188 THEN 'Roman Rule'
    WHEN 10222 THEN 'Unification'
    WHEN 10279 THEN 'Brabant Revolution'
    WHEN 10293 THEN 'Medieval Low Countries'
    WHEN 10296 THEN 'Egypt'
    WHEN 10372 THEN 'Přemyslid Dynasty'
    WHEN 10381 THEN 'Late Ottoman Rule'
    WHEN 10639 THEN '19th Century'
    WHEN 11457 THEN 'Hundred Years’ War'
    WHEN 11464 THEN 'Fourth Republic'
    WHEN 11512 THEN 'Modern Era'
    WHEN 11536 THEN 'Union Era'
    WHEN 11553 THEN 'Late Yugoslavia'
    WHEN 11564 THEN 'Ottoman Reforms'
    WHEN 11578 THEN 'House of Braganza'
    WHEN 11579 THEN 'Monarchy'
    WHEN 51008 THEN 'Early 20th Century'
    WHEN 51013 THEN 'The Twenties'
    WHEN 51017 THEN 'F. D. Roosevelt'
    WHEN 51018 THEN 'Harry Truman'
    WHEN 51082 THEN 'George W. Bush'
    WHEN 51083 THEN 'Barack Obama'
    WHEN 51084 THEN 'Donald Trump'
    WHEN 51085 THEN 'Joe Biden'
END
WHERE concept_id IN (
    3411,3553,8253,9283,10101,10108,10130,10138,10181,10188,
    10222,10279,10293,10296,10372,10381,10639,11457,11464,11512,
    11536,11553,11564,11578,11579,51008,51013,51017,51018,51082,
    51083,51084,51085
);

COMMIT;
