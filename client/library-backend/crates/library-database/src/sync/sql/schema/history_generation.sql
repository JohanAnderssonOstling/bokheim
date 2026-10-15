-- Old numeric checkpoints remain unbound and trigger a full reconciliation.
ALTER TABLE sync_metadata ADD COLUMN pull_generation TEXT;
