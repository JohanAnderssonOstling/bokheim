INSERT INTO external_entity(wikidata_id,kind,label,latitude_e7,longitude_e7)
             VALUES(?1,'place',?2,?3,?4)
             ON CONFLICT(wikidata_id) DO UPDATE SET kind='place',label=excluded.label,latitude_e7=excluded.latitude_e7,longitude_e7=excluded.longitude_e7