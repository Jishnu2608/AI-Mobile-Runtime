//! PROJ-001: workspace trust. Building a project runs its own code (Gradle scripts, npm
//! `postinstall`, Flutter plugins), so the product only builds folders the user trusted once.
//! Trust covers the folder and everything inside it.

use rusqlite::{params, OptionalExtension};

use crate::db::{Db, StorageError};

/// Case-insensitive Windows key for a folder path.
#[must_use]
pub fn path_key(path: &str) -> String {
    let p = path.trim().replace('/', "\\");
    let p = p.strip_prefix(r"\\?\").unwrap_or(&p);
    p.trim_end_matches('\\').to_lowercase()
}

impl Db {
    /// Marks a folder as trusted.
    pub fn trust_project(
        &self,
        path: &str,
        client_id: Option<&str>,
        at_unix: i64,
    ) -> Result<(), StorageError> {
        self.conn().execute(
            "INSERT INTO trusted_projects (path_key, path, trusted_unix, client_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path_key) DO UPDATE SET path = excluded.path, trusted_unix = excluded.trusted_unix, client_id = excluded.client_id",
            params![path_key(path), path, at_unix, client_id],
        )?;
        Ok(())
    }

    /// Removes trust. Returns whether it was trusted.
    pub fn untrust_project(&self, path: &str) -> Result<bool, StorageError> {
        Ok(self.conn().execute(
            "DELETE FROM trusted_projects WHERE path_key = ?1",
            [path_key(path)],
        )? == 1)
    }

    /// Is this folder, or a folder containing it, trusted?
    pub fn is_trusted(&self, path: &str) -> Result<bool, StorageError> {
        let key = path_key(path);
        let mut stmt = self
            .conn()
            .prepare("SELECT path_key FROM trusted_projects")?;
        let keys: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(keys
            .iter()
            .any(|k| key == *k || key.starts_with(&format!("{k}\\"))))
    }

    /// When the folder itself was trusted (exact match only).
    pub fn trusted_since(&self, path: &str) -> Result<Option<i64>, StorageError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT trusted_unix FROM trusted_projects WHERE path_key = ?1",
                [path_key(path)],
                |r| r.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_db_path;

    #[test]
    fn trust_covers_sub_folders_case_insensitively() {
        let db = Db::open(&temp_db_path("trust")).unwrap();
        assert!(!db.is_trusted(r"D:\Work\App").unwrap());
        db.trust_project(r"D:\Work\App\", Some("admin"), 7).unwrap();
        assert!(db.is_trusted(r"d:\work\app").unwrap());
        assert!(db.is_trusted(r"D:\Work\App\android").unwrap());
        assert!(db.is_trusted(r"\\?\D:\Work\App").unwrap());
        assert!(
            !db.is_trusted(r"D:\Work\AppOther").unwrap(),
            "prefix of a name is not a sub-folder"
        );
        assert!(!db.is_trusted(r"D:\Work").unwrap(), "parent is not trusted");
        assert_eq!(db.trusted_since(r"D:/Work/App").unwrap(), Some(7));
        assert!(db.untrust_project(r"D:\WORK\APP").unwrap());
        assert!(!db.is_trusted(r"D:\Work\App\android").unwrap());
    }
}
