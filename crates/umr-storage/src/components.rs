//! Records of SDK components and tools (used by the installer, INS-003, and `umr doctor`).

use rusqlite::{params, OptionalExtension, Row};

use crate::db::{Db, StorageError};

/// Where a component came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Installed by us into the managed root.
    Managed,
    /// Found on the machine and reused read-only.
    User,
    /// Shipped inside another tool (e.g. Android Studio's JBR).
    Bundled,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::User => "user",
            Self::Bundled => "bundled",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "managed" => Some(Self::Managed),
            "user" => Some(Self::User),
            "bundled" => Some(Self::Bundled),
            _ => None,
        }
    }
}

/// One component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// Unique id, e.g. `jdk/temurin@17.0.21` or `android/platforms/android-36`.
    pub id: String,
    pub kind: String,
    pub version: String,
    pub path: String,
    pub source: Source,
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub installed_unix: i64,
    pub verified_unix: Option<i64>,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Component> {
    let source: String = r.get(4)?;
    Ok(Component {
        id: r.get(0)?,
        kind: r.get(1)?,
        version: r.get(2)?,
        path: r.get(3)?,
        source: Source::parse(&source).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                format!("bad source '{source}'").into(),
            )
        })?,
        sha256: r.get(5)?,
        size_bytes: r.get(6)?,
        installed_unix: r.get(7)?,
        verified_unix: r.get(8)?,
    })
}

const COLUMNS: &str =
    "id, kind, version, path, source, sha256, size_bytes, installed_unix, verified_unix";

impl Db {
    /// Inserts or replaces a component record.
    pub fn upsert_component(&self, c: &Component) -> Result<(), StorageError> {
        self.conn().execute(
            &format!(
                "INSERT INTO components ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, version=excluded.version, path=excluded.path,
                   source=excluded.source, sha256=excluded.sha256, size_bytes=excluded.size_bytes,
                   installed_unix=excluded.installed_unix, verified_unix=excluded.verified_unix"
            ),
            params![c.id, c.kind, c.version, c.path, c.source.as_str(), c.sha256, c.size_bytes, c.installed_unix, c.verified_unix],
        )?;
        Ok(())
    }

    /// One component by id.
    pub fn component(&self, id: &str) -> Result<Option<Component>, StorageError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM components WHERE id = ?1"),
                [id],
                from_row,
            )
            .optional()?)
    }

    /// All components, optionally of one kind, ordered by id.
    pub fn components(&self, kind: Option<&str>) -> Result<Vec<Component>, StorageError> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {COLUMNS} FROM components WHERE (?1 IS NULL OR kind = ?1) ORDER BY id"
        ))?;
        let rows = stmt.query_map([kind], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Records a successful functional check.
    pub fn mark_component_verified(&self, id: &str, at_unix: i64) -> Result<bool, StorageError> {
        Ok(self.conn().execute(
            "UPDATE components SET verified_unix = ?2 WHERE id = ?1",
            params![id, at_unix],
        )? == 1)
    }

    /// Deletes a component record (the files are handled by the installer). Returns whether it existed.
    pub fn remove_component(&self, id: &str) -> Result<bool, StorageError> {
        Ok(self
            .conn()
            .execute("DELETE FROM components WHERE id = ?1", [id])?
            == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_db_path;

    fn jdk() -> Component {
        Component {
            id: "jdk/temurin@17.0.21".into(),
            kind: "jdk".into(),
            version: "17.0.21".into(),
            path: r"D:\UMR\jdks\temurin-17.0.21".into(),
            source: Source::Managed,
            sha256: Some("98518ecf".into()),
            size_bytes: Some(190_000_000),
            installed_unix: 1_794_000_000,
            verified_unix: None,
        }
    }

    #[test]
    fn crud_round_trip() {
        let p = temp_db_path("components");
        let db = Db::open(&p).unwrap();
        db.upsert_component(&jdk()).unwrap();
        db.upsert_component(&Component {
            id: "node@26.13.0".into(),
            kind: "node".into(),
            source: Source::User,
            sha256: None,
            ..jdk()
        })
        .unwrap();

        assert_eq!(db.component("jdk/temurin@17.0.21").unwrap(), Some(jdk()));
        assert_eq!(db.components(None).unwrap().len(), 2);
        assert_eq!(db.components(Some("node")).unwrap()[0].source, Source::User);

        assert!(db
            .mark_component_verified("jdk/temurin@17.0.21", 1_794_000_100)
            .unwrap());
        assert_eq!(
            db.component("jdk/temurin@17.0.21")
                .unwrap()
                .unwrap()
                .verified_unix,
            Some(1_794_000_100)
        );

        let mut updated = jdk();
        updated.path = r"E:\other".into();
        db.upsert_component(&updated).unwrap();
        assert_eq!(
            db.component(&updated.id).unwrap().unwrap().path,
            r"E:\other"
        );

        assert!(db.remove_component("node@26.13.0").unwrap());
        assert!(!db.remove_component("node@26.13.0").unwrap());
        assert_eq!(db.components(None).unwrap().len(), 1);
        drop(db);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn invalid_source_is_rejected_by_schema() {
        let p = temp_db_path("components-check");
        let db = Db::open(&p).unwrap();
        let r = db.conn().execute(
            "INSERT INTO components (id, kind, version, path, source, installed_unix) VALUES ('x','k','1','p','bogus',0)",
            [],
        );
        assert!(r.is_err(), "CHECK constraint must reject unknown source");
        drop(db);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
