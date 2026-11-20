//! Ordered schema migrations. Migration `n` brings the schema from version `n-1` to `n`.
//!
//! Rules:
//! - Never edit a migration that has shipped; add a new one.
//! - Each migration runs in its own transaction together with the `user_version` bump.

/// One migration step.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u32,
    pub description: &'static str,
    pub sql: &'static str,
}

/// All migrations, in order. Version numbers must be 1, 2, 3, … without gaps.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "initial schema: meta, components, env_scans, audit_log",
        sql: r"
        -- Key/value facts about this database (created_at, product version that created it, ...).
        CREATE TABLE meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        ) STRICT;

        -- SDK components and tools managed (or reused read-only) by the installer.
        CREATE TABLE components (
            id            TEXT PRIMARY KEY,           -- e.g. 'jdk/temurin@17.0.21'
            kind          TEXT NOT NULL,              -- jdk | android-sdk-package | flutter | node | ...
            version       TEXT NOT NULL,
            path          TEXT NOT NULL,
            source        TEXT NOT NULL CHECK (source IN ('managed', 'user', 'bundled')),
            sha256        TEXT,                       -- of the verified download, when managed
            size_bytes    INTEGER,
            installed_unix INTEGER NOT NULL,
            verified_unix  INTEGER                    -- last successful functional check
        ) STRICT;
        CREATE INDEX components_kind ON components(kind);

        -- History of environment readiness scans (ENV-006); report stored as JSON.
        CREATE TABLE env_scans (
            id          INTEGER PRIMARY KEY,
            scanned_unix INTEGER NOT NULL,
            report_json TEXT NOT NULL
        ) STRICT;

        -- Security-relevant events (logins, client changes, shutdowns, installs).
        CREATE TABLE audit_log (
            id        INTEGER PRIMARY KEY,
            at_unix   INTEGER NOT NULL,
            client_id TEXT,
            action    TEXT NOT NULL,
            detail    TEXT
        ) STRICT;
        CREATE INDEX audit_log_at ON audit_log(at_unix);

        INSERT INTO meta(key, value) VALUES ('created_unix', CAST(strftime('%s','now') AS TEXT));
    ",
    },
    Migration {
        version: 2,
        description: "installer: install_journal, licence_acceptances",
        sql: r"
        -- INS-003: one row per install transaction; survives crashes so the Core can finish
        -- or roll back half-done installs on the next start.
        CREATE TABLE install_journal (
            id           TEXT PRIMARY KEY,          -- transaction id
            component    TEXT NOT NULL,             -- component id, e.g. 'jdk/temurin@17.0.21'
            target       TEXT NOT NULL,             -- final folder
            staging      TEXT,                      -- staging folder, if any
            state        TEXT NOT NULL CHECK (state IN
                           ('started','downloaded','extracted','committing','committed','failed','rolled-back')),
            error        TEXT,
            started_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX install_journal_state ON install_journal(state);

        -- INS-004: licences the user accepted, bound to the exact text shown (SHA-256).
        CREATE TABLE licence_acceptances (
            licence_id    TEXT NOT NULL,
            text_sha256   TEXT NOT NULL,
            accepted_unix INTEGER NOT NULL,
            client_id     TEXT,
            PRIMARY KEY (licence_id, text_sha256)
        ) STRICT;
    ",
    },
    Migration {
        version: 3,
        description: "workspace trust: trusted_projects",
        sql: r"
        -- PROJ-001: folders the user trusts to run their build scripts (Gradle, npm, Flutter).
        CREATE TABLE trusted_projects (
            path_key     TEXT PRIMARY KEY,           -- normalised (lower-case, backslashes, no trailing slash)
            path         TEXT NOT NULL,              -- as shown to the user
            trusted_unix INTEGER NOT NULL,
            client_id    TEXT
        ) STRICT;
    ",
    },
    Migration {
        version: 4,
        description: "session recovery journal: sessions, session_events",
        sql: r"
        -- Step 2.6: runtime sessions and every state change, for recovery after a Core restart.
        CREATE TABLE sessions (
            id           TEXT PRIMARY KEY,
            profile      TEXT NOT NULL,
            avd          TEXT NOT NULL,
            project      TEXT,
            app          TEXT,
            state        TEXT NOT NULL,
            serial       TEXT,
            detail       TEXT,
            created_unix INTEGER NOT NULL,
            updated_unix INTEGER NOT NULL
        ) STRICT;
        CREATE TABLE session_events (
            seq          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id   TEXT NOT NULL REFERENCES sessions(id),
            at_unix      INTEGER NOT NULL,
            from_state   TEXT NOT NULL,
            event        TEXT NOT NULL,
            to_state     TEXT NOT NULL,
            detail       TEXT
        ) STRICT;
        CREATE INDEX session_events_by_session ON session_events (session_id, seq);
    ",
    },
];

/// Highest schema version this build understands.
#[must_use]
pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_contiguous_from_one() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                m.version as usize,
                i + 1,
                "migration {} out of order",
                m.description
            );
        }
    }
}
