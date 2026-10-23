-- Fingerprint of the payload that first claimed a client eventId, so a later copy with a
-- different payload is counted as a dedup conflict (0002). Rows from format 1 have none.
ALTER TABLE client_event_ids ADD COLUMN fingerprint BLOB;
