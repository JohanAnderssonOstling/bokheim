SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type IN ('table','view','trigger','index') AND name NOT LIKE 'sqlite_%')
