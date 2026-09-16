INSERT INTO external_entity(wikidata_id,kind,label,birth_year,death_year,fictional)
             VALUES(?1,'person',?2,?3,?4,?5)
             ON CONFLICT(wikidata_id) DO UPDATE SET kind='person',label=excluded.label,birth_year=excluded.birth_year,death_year=excluded.death_year,fictional=excluded.fictional