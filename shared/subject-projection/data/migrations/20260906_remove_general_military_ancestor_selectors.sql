-- The U schedule is completely and mutually exclusively partitioned by the
-- existing General Military Studies leaves. Retain only the exact class marker.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector
WHERE concept_id=5260 AND system_id='lcc' AND selector<>'U';
COMMIT;
