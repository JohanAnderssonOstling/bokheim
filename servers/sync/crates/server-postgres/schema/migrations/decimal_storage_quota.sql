-- Match the decimal GB units shown by clients. Only replace the previous
-- default allowance; preserve custom quotas and all usage/reservation records.
ALTER TABLE user_storage_account ALTER COLUMN quota_bytes SET DEFAULT 5000000000;
UPDATE user_storage_account SET quota_bytes=5000000000 WHERE quota_bytes=5368709120;

ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=13;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=13);
