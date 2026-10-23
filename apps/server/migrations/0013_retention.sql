-- Configurable event retention (decision 0016). Values are the protocol `Retention` codes. The
-- installation default applies to every source without its own value; sites set theirs with
-- `sites.updateRetention`, links and pixels only through an import. Control database only:
-- analytical storage, pending inboxes and existing backups are unaffected.

ALTER TABLE installation_settings ADD COLUMN event_retention TEXT NOT NULL DEFAULT '1y'
    CHECK (event_retention IN ('1y', '2y', '3y', '5y', '10y', 'forever'));

ALTER TABLE sites ADD COLUMN event_retention TEXT
    CHECK (event_retention IN ('1y', '2y', '3y', '5y', '10y', 'forever'));

ALTER TABLE links ADD COLUMN event_retention TEXT
    CHECK (event_retention IN ('1y', '2y', '3y', '5y', '10y', 'forever'));

ALTER TABLE pixels ADD COLUMN event_retention TEXT
    CHECK (event_retention IN ('1y', '2y', '3y', '5y', '10y', 'forever'));
