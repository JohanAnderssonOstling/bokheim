CREATE TEMP TABLE geographic_class(
                     class_id INTEGER PRIMARY KEY
                 ) WITHOUT ROWID;
                 INSERT OR IGNORE INTO geographic_class(class_id) VALUES
                     (486972),  /* human settlement */
                     (82794),   /* geographic region */
                     (6256),    /* country */
                     (5107),    /* continent */
                     (618123);  /* geographical object */
                 WITH RECURSIVE descendants(class_id) AS (
                     SELECT class_id FROM geographic_class
                     UNION
                     SELECT relation.child_id
                     FROM wikidata_subclass AS relation
                     JOIN descendants AS parent ON parent.class_id=relation.parent_id
                 )
                 INSERT OR IGNORE INTO geographic_class(class_id)
                     SELECT class_id FROM descendants;
                 INSERT OR IGNORE INTO wikidata_candidate(kind,normalized_name,wikidata_id)
                     SELECT 1,candidate.normalized_name,candidate.wikidata_id
                     FROM wikidata_place_candidate AS candidate
                     JOIN wikidata_instance AS instance ON instance.wikidata_id=candidate.wikidata_id
                     JOIN geographic_class ON geographic_class.class_id=instance.class_id;
                 DROP TABLE geographic_class;
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'place_candidate_headings',COUNT(DISTINCT normalized_name) FROM wikidata_candidate WHERE kind=1;
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'place_ambiguous_headings',COUNT(*) FROM (SELECT normalized_name FROM wikidata_candidate WHERE kind=1 GROUP BY normalized_name HAVING COUNT(*)>1);
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'person_candidate_headings',COUNT(DISTINCT normalized_name) FROM wikidata_candidate WHERE kind=2;
                 INSERT OR REPLACE INTO metadata_metric(name,value)
                     SELECT 'person_ambiguous_headings',COUNT(*) FROM (SELECT normalized_name FROM wikidata_candidate WHERE kind=2 GROUP BY normalized_name HAVING COUNT(*)>1);
                 UPDATE edition_entity_evidence AS evidence SET wikidata_id=(
             SELECT MIN(candidate.wikidata_id) FROM wikidata_candidate AS candidate
             WHERE candidate.kind=evidence.kind AND candidate.normalized_name=evidence.normalized_name
             HAVING COUNT(*)=1
         );
         UPDATE work_entity_evidence AS evidence SET wikidata_id=(
             SELECT MIN(candidate.wikidata_id) FROM wikidata_candidate AS candidate
             WHERE candidate.kind=evidence.kind AND candidate.normalized_name=evidence.normalized_name
             HAVING COUNT(*)=1
         );
         CREATE TEMP TABLE retained_entity(
             wikidata_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         INSERT OR IGNORE INTO retained_entity(wikidata_id)
             SELECT wikidata_id FROM edition_entity_evidence WHERE wikidata_id IS NOT NULL
             UNION
             SELECT wikidata_id FROM work_entity_evidence WHERE wikidata_id IS NOT NULL;
         WITH RECURSIVE place_closure(wikidata_id) AS (
             SELECT retained.wikidata_id
             FROM retained_entity AS retained
             JOIN entity ON entity.wikidata_id=retained.wikidata_id
             WHERE entity.kind=1
             UNION
             SELECT relation.parent_id
             FROM place_parent AS relation
             JOIN place_closure AS retained ON retained.wikidata_id=relation.place_id
             JOIN entity ON entity.wikidata_id=relation.parent_id AND entity.kind=1
             UNION
             SELECT relation.country_id
             FROM place_country AS relation
             JOIN place_closure AS retained ON retained.wikidata_id=relation.place_id
             JOIN entity ON entity.wikidata_id=relation.country_id AND entity.kind=1
         )
         INSERT OR IGNORE INTO retained_entity(wikidata_id)
             SELECT wikidata_id FROM place_closure;
         DELETE FROM entity_alias WHERE wikidata_id NOT IN (SELECT wikidata_id FROM retained_entity);
         DELETE FROM place_parent
             WHERE place_id NOT IN (SELECT wikidata_id FROM retained_entity)
                OR parent_id NOT IN (SELECT wikidata_id FROM retained_entity);
         DELETE FROM place_country
             WHERE place_id NOT IN (SELECT wikidata_id FROM retained_entity)
                OR country_id NOT IN (SELECT wikidata_id FROM retained_entity);
         DELETE FROM entity WHERE wikidata_id NOT IN (SELECT wikidata_id FROM retained_entity);
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'place_evidence',COUNT(*) FROM edition_entity_evidence WHERE kind=1;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'person_evidence',COUNT(*) FROM edition_entity_evidence WHERE kind=2;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'work_place_evidence',COUNT(*) FROM work_entity_evidence WHERE kind=1;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'work_person_evidence',COUNT(*) FROM work_entity_evidence WHERE kind=2;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'resolved_place_evidence',COUNT(*) FROM (
                 SELECT wikidata_id FROM edition_entity_evidence WHERE kind=1 AND wikidata_id IS NOT NULL
                 UNION ALL SELECT wikidata_id FROM work_entity_evidence WHERE kind=1 AND wikidata_id IS NOT NULL
             );
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'resolved_person_evidence',COUNT(*) FROM (
                 SELECT wikidata_id FROM edition_entity_evidence WHERE kind=2 AND wikidata_id IS NOT NULL
                 UNION ALL SELECT wikidata_id FROM work_entity_evidence WHERE kind=2 AND wikidata_id IS NOT NULL
             );
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_places',COUNT(*) FROM entity WHERE kind=1;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_people',COUNT(*) FROM entity WHERE kind=2;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_aliases',COUNT(*) FROM entity_alias;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_parent_edges',COUNT(*) FROM place_parent;
         INSERT OR REPLACE INTO metadata_metric(name,value)
             SELECT 'retained_country_edges',COUNT(*) FROM place_country;
         DROP TABLE retained_entity;
         DROP TABLE wikidata_candidate;
         DROP TABLE wikidata_place_candidate;
         DROP TABLE wikidata_instance;
         DROP TABLE wikidata_subclass;
