-- Preserve dates in named eras, events, dynasties, republics, and
-- administrations. Only labels containing "Century" use the shortened form.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH renames(concept_id, old_label, new_label) AS (VALUES
    (10344, 'Age of Tyrants', 'Archaic Era, c. 800–500 BC'),
    (8253, 'Ancien Régime', 'Ancien Régime, 1715–1789'),
    (9283, 'Napoleonic Era', 'Napoleonic Era, 1796–1815'),
    (10101, 'Bourbon Era', 'Bourbon Era, 1700–1808'),
    (10108, 'Contemporary Era', 'Contemporary Era, 2000–Present'),
    (10130, 'Early Modern', 'Early Modern Era, 1603–1707'),
    (10181, 'Enlightenment Era', 'Enlightenment Era, 1740–1789'),
    (10188, 'Roman Rule', 'Roman Rule, 146 BC–323 AD'),
    (10222, 'Unification', 'Unification, 1815–1871'),
    (10279, 'Brabant Revolution', 'Brabant Revolution, 1789–1790'),
    (10293, 'Medieval Low Countries', 'Medieval Low Countries, 843–1384'),
    (10296, 'Egypt', 'Egypt, 1798–1879'),
    (10372, 'Přemyslid Dynasty', 'Přemyslid Dynasty, c. 900–1310'),
    (10381, 'Late Ottoman Rule', 'Late Ottoman Rule, 1606–1699'),
    (11457, 'Hundred Years’ War', 'Hundred Years’ War, 1337–1453'),
    (11464, 'Fourth Republic', 'Fourth Republic, 1946–1958'),
    (11512, 'Modern Era', 'Modern Era, 1808–1906'),
    (11536, 'Union Era', 'Union Era, 1814–1905'),
    (11553, 'Late Yugoslavia', 'Late Yugoslavia, 1980–1992'),
    (11564, 'Ottoman Reforms', 'Ottoman Reforms, 1789–1861'),
    (11578, 'House of Braganza', 'House of Braganza, 1640–1816'),
    (11579, 'Monarchy', 'Monarchy, 1816–1908'),
    (51013, 'The Twenties', 'The Twenties, 1919–1933'),
    (51017, 'F. D. Roosevelt', 'F. D. Roosevelt, 1933–1945'),
    (51018, 'Harry Truman', 'Harry Truman, 1945–1953'),
    (51082, 'George W. Bush', 'George W. Bush, 2001–2009'),
    (51083, 'Barack Obama', 'Barack Obama, 2009–2017'),
    (51084, 'Donald Trump', 'Donald Trump, 2017–2021'),
    (51085, 'Joe Biden', 'Joe Biden, 2021–2025')
)
INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
SELECT concept_id, old_label, 0 FROM renames;

UPDATE concept
SET preferred_label = CASE concept_id
    WHEN 10344 THEN 'Archaic Era, c. 800–500 BC'
    WHEN 8253 THEN 'Ancien Régime, 1715–1789'
    WHEN 9283 THEN 'Napoleonic Era, 1796–1815'
    WHEN 10101 THEN 'Bourbon Era, 1700–1808'
    WHEN 10108 THEN 'Contemporary Era, 2000–Present'
    WHEN 10130 THEN 'Early Modern Era, 1603–1707'
    WHEN 10181 THEN 'Enlightenment Era, 1740–1789'
    WHEN 10188 THEN 'Roman Rule, 146 BC–323 AD'
    WHEN 10222 THEN 'Unification, 1815–1871'
    WHEN 10279 THEN 'Brabant Revolution, 1789–1790'
    WHEN 10293 THEN 'Medieval Low Countries, 843–1384'
    WHEN 10296 THEN 'Egypt, 1798–1879'
    WHEN 10372 THEN 'Přemyslid Dynasty, c. 900–1310'
    WHEN 10381 THEN 'Late Ottoman Rule, 1606–1699'
    WHEN 11457 THEN 'Hundred Years’ War, 1337–1453'
    WHEN 11464 THEN 'Fourth Republic, 1946–1958'
    WHEN 11512 THEN 'Modern Era, 1808–1906'
    WHEN 11536 THEN 'Union Era, 1814–1905'
    WHEN 11553 THEN 'Late Yugoslavia, 1980–1992'
    WHEN 11564 THEN 'Ottoman Reforms, 1789–1861'
    WHEN 11578 THEN 'House of Braganza, 1640–1816'
    WHEN 11579 THEN 'Monarchy, 1816–1908'
    WHEN 51013 THEN 'The Twenties, 1919–1933'
    WHEN 51017 THEN 'F. D. Roosevelt, 1933–1945'
    WHEN 51018 THEN 'Harry Truman, 1945–1953'
    WHEN 51082 THEN 'George W. Bush, 2001–2009'
    WHEN 51083 THEN 'Barack Obama, 2009–2017'
    WHEN 51084 THEN 'Donald Trump, 2017–2021'
    WHEN 51085 THEN 'Joe Biden, 2021–2025'
END
WHERE concept_id IN (10344,8253,9283,10101,10108,10130,10181,10188,10222,
    10279,10293,10296,10372,10381,11457,11464,11512,11536,11553,11564,
    11578,11579,51013,51017,51018,51082,51083,51084,51085);

COMMIT;
