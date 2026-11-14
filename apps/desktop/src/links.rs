//! Opening things from the wizard and the preview window (Phase 9.6, WEB-012).
//!
//! * docs pages, in the user's browser: a fixed list of pages and anchors, so the web view can
//!   never open an arbitrary address. Until the website has a domain (W2) the pages are the
//!   user guides in the public repository, which the website shows too.
//! * the live preview window: this program again with `--preview`, as `umr run` does.

use std::process::{Command, Stdio};

/// Where the user guides are read today (the public repository; the website needs W2).
const DOCS_BASE: &str = "https://github.com/Jishnu2608/AI-Mobile-Runtime/blob/main/docs/user/";

/// The pages the app may open.
const PAGES: &[&str] = &[
    "getting-started",
    "agents",
    "troubleshooting",
    "uninstall-and-data",
    "privacy-and-security",
    "cli-reference",
];

/// The docs address for a page and an optional section, or why it is refused.
pub fn docs_url(page: &str, anchor: Option<&str>) -> Result<String, String> {
    if !PAGES.contains(&page) {
        return Err(format!("unknown docs page '{page}'"));
    }
    let mut url = format!("{DOCS_BASE}{page}.md");
    if let Some(a) = anchor.filter(|a| !a.is_empty()) {
        if a.len() > 80
            || !a
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(format!("invalid section '{a}'"));
        }
        url.push('#');
        url.push_str(a);
    }
    Ok(url)
}

/// Opens a docs page in the default browser.
pub fn open_docs(page: &str, anchor: Option<&str>) -> Result<(), String> {
    let url = docs_url(page, anchor)?;
    // explorer.exe hands an https address to the default browser; no shell is involved
    Command::new("explorer.exe")
        .arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot open {url}: {e}"))
}

/// Starts the docked preview window (a second instance of this program with `--preview`).
pub fn open_preview() -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(me)
        .args(["--preview", "--dock", "right"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot open the preview window: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_pages_and_plain_anchors() {
        assert!(docs_url("getting-started", Some("3-your-first-app"))
            .unwrap()
            .ends_with("docs/user/getting-started.md#3-your-first-app"));
        assert!(docs_url("troubleshooting", None)
            .unwrap()
            .ends_with("troubleshooting.md"));
        assert!(docs_url("../secrets", None).is_err());
        assert!(docs_url("agents", Some("x\" & calc")).is_err());
        assert!(docs_url("agents", Some("Upper")).is_err());
    }
}
