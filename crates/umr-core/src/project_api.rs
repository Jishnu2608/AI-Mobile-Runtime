//! PROJ-001: `project.detect` and `project.trust`, plus the trust gate future build methods
//! (Phase 3: `umr run`, rebuild, reload) call before running any project code.

use std::path::Path;

use umr_api::methods::{ProjectPathParams, TrustParams, TrustResult};
use umr_api::rpc::{codes, Id, Response, RpcError};

use crate::api_server::{internal, ok, ApiState};

/// AUD-P2-19: what is wrong with a project path, in the user's words (`None`: a folder).
#[must_use]
pub fn folder_problem(path: &str) -> Option<String> {
    let t = path.trim();
    if t.is_empty() {
        return Some("Choose your app's folder first.".into());
    }
    let p = Path::new(t);
    if !p.is_absolute() {
        return Some(format!(
            "Use the full path of the folder, starting with the drive (for example D:\\Work\\my-app), not '{t}'."
        ));
    }
    if p.is_file() {
        return Some(format!(
            "'{t}' is a file. Choose the folder that holds it instead."
        ));
    }
    if !p.is_dir() {
        return Some(format!(
            "This folder does not exist: '{t}'. Check the spelling, or choose it with the folder picker."
        ));
    }
    None
}

pub(crate) fn check_dir(path: &str) -> Result<(), RpcError> {
    folder_problem(path).map_or(Ok(()), |m| Err(RpcError::new(codes::INVALID_PARAMS, m)))
}

pub fn detect(state: &ApiState, id: Id, p: &ProjectPathParams) -> Response {
    if let Err(e) = check_dir(&p.path) {
        return Response::err(Some(id), e);
    }
    let detection = umr_resolver::detect::detect(Path::new(&p.path));
    let trusted = state
        .db
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_trusted(&p.path)
        .unwrap_or(false);
    ok(
        id,
        &serde_json::json!({ "detection": detection, "trusted": trusted }),
    )
}

pub fn trust(state: &ApiState, id: Id, client: Option<&str>, p: &TrustParams) -> Response {
    if let Err(e) = check_dir(&p.path) {
        return Response::err(Some(id), e);
    }
    let db = state
        .db
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = umr_installer::now_unix();
    let result = if p.trusted {
        db.trust_project(&p.path, client, now).map(|()| true)
    } else {
        db.untrust_project(&p.path).map(|_| false)
    };
    match result {
        Ok(trusted) => {
            let action = if trusted {
                "project.trust"
            } else {
                "project.untrust"
            };
            let _ = db.audit(now, client, action, Some(&p.path));
            tracing::info!(path = %p.path, trusted, "workspace trust changed");
            ok(
                id,
                &TrustResult {
                    path: p.path.clone(),
                    trusted,
                },
            )
        }
        Err(e) => internal(id, &e),
    }
}

/// Gate for anything that runs project code. `Err` carries [`codes::UNTRUSTED`].
pub fn require_trusted(state: &ApiState, path: &str) -> Result<(), RpcError> {
    let trusted = state
        .db
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_trusted(path)
        .map_err(|e| RpcError::new(codes::INTERNAL_ERROR, e.to_string()))?;
    if trusted {
        Ok(())
    } else {
        Err(RpcError::new(
            codes::UNTRUSTED,
            format!("'{path}' is not trusted yet. Building runs the project's own scripts (Gradle, npm, Flutter); trust the folder first (`umr trust`)."),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    #[test]
    fn folder_problems_say_what_is_wrong() {
        let tmp = TempRoot::new("folder-problem");
        let dir = tmp.path().display().to_string();
        assert_eq!(folder_problem(&dir), None);
        let missing = tmp.path().join("does-not-exist").display().to_string();
        assert!(folder_problem(&missing)
            .unwrap()
            .starts_with("This folder does not exist"));
        let file = tmp.path().join("pubspec.yaml");
        std::fs::write(&file, "name: x").unwrap();
        assert!(folder_problem(&file.display().to_string())
            .unwrap()
            .contains("is a file"));
        assert!(folder_problem("my-app")
            .unwrap()
            .starts_with("Use the full path"));
        assert!(folder_problem("  ").is_some());
    }

    #[test]
    fn trust_gate() {
        let tmp = TempRoot::new("trust-gate");
        let auth =
            crate::auth::AuthStore::load(&tmp.path().join("clients.json"), "umr_admin").unwrap();
        let state = ApiState::new(
            tmp.path().to_path_buf(),
            auth,
            umr_storage::Db::open_in(tmp.path()).unwrap(),
        );
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.display().to_string();
        assert_eq!(
            require_trusted(&state, &path).unwrap_err().code,
            codes::UNTRUSTED
        );
        let r = trust(
            &state,
            Id::Num(1),
            Some("admin"),
            &TrustParams {
                path: path.clone(),
                trusted: true,
            },
        );
        assert!(r.error.is_none());
        require_trusted(&state, &path).unwrap();
        require_trusted(&state, &proj.join("android").display().to_string()).unwrap();
        let r = trust(
            &state,
            Id::Num(2),
            None,
            &TrustParams {
                path: path.clone(),
                trusted: false,
            },
        );
        assert!(r.error.is_none());
        assert!(require_trusted(&state, &path).is_err());
        let r = trust(
            &state,
            Id::Num(3),
            None,
            &TrustParams {
                path: "relative".into(),
                trusted: true,
            },
        );
        assert_eq!(r.error.unwrap().code, codes::INVALID_PARAMS);
    }
}
