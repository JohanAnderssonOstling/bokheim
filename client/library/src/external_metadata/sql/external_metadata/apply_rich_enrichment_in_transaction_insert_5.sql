INSERT INTO external_entity(wikidata_id,kind,label) VALUES(?1,?2,?3)
                             ON CONFLICT(wikidata_id) DO UPDATE SET kind=excluded.kind,label=excluded.label