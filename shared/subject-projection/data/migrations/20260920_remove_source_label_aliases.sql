-- Preferred concept names are authoritative. Retain this empty compatibility
-- table in the editable format-8 source; client format 11 drops it entirely.
-- Readers ignore aliases in older snapshots and incoming legacy payloads.
DELETE FROM source_label;
