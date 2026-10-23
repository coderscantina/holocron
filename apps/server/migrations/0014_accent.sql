-- The accent colour preset per person. Values are the protocol `Accent` codes; NULL is the default.

ALTER TABLE user_preferences ADD COLUMN accent TEXT
    CHECK (accent IN ('indigo', 'blue', 'violet', 'rose', 'orange', 'amber', 'emerald', 'cyan'));
