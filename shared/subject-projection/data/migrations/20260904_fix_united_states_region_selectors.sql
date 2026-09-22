-- Remove narrow river-valley selectors from retained broad U.S. regions and
-- keep the Southern Civil War selector only on its specific period concept.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- F516-F520 covers the Ohio River and Valley, which is not retained as a
-- curated subject. It must not classify books as both Midwest and South.
DELETE FROM source_selector
WHERE concept_id IN (
          'unified:413b2289-3f2f-5000-8218-a8bca93ba70b',
          'unified:508a9bbd-bc3f-50fe-97f7-d1c27c8aacdc'
      )
  AND selector IN (
      'F516..F520@2','F517..F517@5','F518..F518@5',
      'F519..F519@5','F520..F520@5','F520.5..F520.6@4'
  );

-- F598 is the Missouri River and Valley, another deliberately omitted narrow
-- geographic feature. Do not force it into Midwest or West.
DELETE FROM source_selector
WHERE concept_id IN (
          'unified:413b2289-3f2f-5000-8218-a8bca93ba70b',
          'unified:67e96244-92a5-55e7-8801-c05d8911dea6'
      )
  AND selector IN ('F598..F598@2','F598..F598@4');

-- F214 is specifically the Civil War period of the South. Its period concept
-- supplies the browse hierarchy; the broad South concept needs no duplicate.
DELETE FROM source_selector
WHERE concept_id='unified:508a9bbd-bc3f-50fe-97f7-d1c27c8aacdc'
  AND selector='F214..F214@5';

COMMIT;
