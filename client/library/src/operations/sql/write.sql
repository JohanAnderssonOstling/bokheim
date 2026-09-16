INSERT INTO sync_metadata(key,value) VALUES('local_library_operation_v1',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value!=excluded.value
