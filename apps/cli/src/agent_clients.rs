//! Phase 10.1 (CON-001): the registry of coding-agent clients the runtime can connect to.
//!
//! One entry per client: where it reads its MCP server list (project and user scope), the file
//! format, where agent instructions go, and how to tell it is installed. Formats were checked
//! against each client's own documentation on the date in `verified` (details and sources in
//! `docs/agents/clients.md`). `tumr agent setup` (`agent_setup.rs`) writes every client whose
//! `setup` is true.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// How a client's config file holds its MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// JSON object under `key`; `typed` adds `"type": "stdio"` to the entry.
    Json { key: &'static str, typed: bool },
    /// Codex `config.toml`: one `[mcp_servers.<name>]` table, `command` = stdio.
    CodexToml,
}

/// Where a file lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Relative to the project folder.
    Project(&'static str),
    /// Relative to `%USERPROFILE%`.
    Home(&'static str),
    /// Relative to `%APPDATA%`.
    AppData(&'static str),
    /// Relative to `%CODEX_HOME%` (default `%USERPROFILE%\.codex`).
    CodexHome(&'static str),
    /// Claude Desktop's config. Current packaged builds keep it in
    /// `%LOCALAPPDATA%\Claude-Data\<package>`; older ones in the MSIX package's roaming folder or in
    /// `%APPDATA%\Claude` (see [`claude_desktop_dir`]).
    ClaudeDesktop,
}

/// Where the agent instructions (MCP-005) go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instructions {
    /// A file of its own, relative to the project.
    File(&'static str),
    /// A marked section in a shared project file.
    Section(&'static str),
    /// No file: the client gets the MCP `instructions` field only.
    McpOnly,
}

/// One client.
#[derive(Debug, Clone, Copy)]
pub struct Client {
    pub id: &'static str,
    pub name: &'static str,
    pub format: Format,
    pub project_config: Option<Place>,
    pub user_config: Option<Place>,
    pub instructions: Instructions,
    /// Commands on `PATH` that show the client is installed.
    pub detect_commands: &'static [&'static str],
    /// Folders that show the client is installed.
    pub detect_places: &'static [Place],
    /// `tumr agent setup <id>` can write it today.
    pub setup: bool,
    /// Date the format was checked against the client's documentation.
    pub verified: &'static str,
    pub notes: &'static str,
}

/// Every client, in the order they are listed.
pub const CLIENTS: [Client; 6] = [
    Client {
        id: "claude-code",
        name: "Claude Code",
        format: Format::Json { key: "mcpServers", typed: true },
        project_config: Some(Place::Project(".mcp.json")),
        user_config: Some(Place::Home(".claude.json")),
        instructions: Instructions::File(".claude/skills/tumr-runtime/SKILL.md"),
        detect_commands: &["claude"],
        detect_places: &[Place::Home(".claude")],
        setup: true,
        verified: "2026-11-14",
        notes: "Project scope is shared through the repository and asks once for approval; user scope lives in ~/.claude.json. When a name exists in several scopes, local beats project beats user.",
    },
    Client {
        id: "codex",
        name: "Codex",
        format: Format::CodexToml,
        project_config: Some(Place::Project(".codex/config.toml")),
        user_config: Some(Place::CodexHome("config.toml")),
        instructions: Instructions::Section("AGENTS.md"),
        detect_commands: &["codex"],
        detect_places: &[Place::CodexHome("")],
        setup: true,
        verified: "2026-11-14",
        notes: "A `command` key selects stdio, a `url` key HTTP; mixing them is rejected. Project config is read only in trusted projects.",
    },
    Client {
        id: "cursor",
        name: "Cursor",
        format: Format::Json { key: "mcpServers", typed: true },
        project_config: Some(Place::Project(".cursor/mcp.json")),
        user_config: Some(Place::Home(".cursor/mcp.json")),
        instructions: Instructions::File(".cursor/rules/tumr-runtime.mdc"),
        detect_commands: &["cursor"],
        detect_places: &[Place::Home(".cursor")],
        setup: true,
        verified: "2026-11-14",
        notes: "The command must be on PATH or a full path. Rules are .mdc files with front matter; `alwaysApply: true` loads them in every chat.",
    },
    Client {
        id: "claude-desktop",
        name: "Claude Desktop",
        format: Format::Json { key: "mcpServers", typed: false },
        project_config: None,
        user_config: Some(Place::ClaudeDesktop),
        instructions: Instructions::McpOnly,
        detect_commands: &[],
        detect_places: &[Place::AppData("Claude")],
        setup: true,
        verified: "2026-11-14",
        notes: "Local stdio servers only in this file (remote ones go through Settings > Connectors). User-wide, so project approvals still apply. Restart the app after a change.",
    },
    Client {
        id: "vscode",
        name: "VS Code (agent mode)",
        format: Format::Json { key: "servers", typed: true },
        project_config: Some(Place::Project(".vscode/mcp.json")),
        user_config: Some(Place::AppData("Code/User/mcp.json")),
        instructions: Instructions::File(".github/instructions/tumr-runtime.instructions.md"),
        detect_commands: &["code"],
        detect_places: &[Place::AppData("Code/User")],
        setup: true,
        verified: "2026-11-14",
        notes: "`type` is required. The Copilot harness also reads a workspace .mcp.json (`mcpServers`), the same file Claude Code uses.",
    },
    Client {
        id: "antigravity",
        name: "Antigravity",
        format: Format::Json { key: "mcpServers", typed: false },
        project_config: Some(Place::Project(".agents/mcp_config.json")),
        user_config: Some(Place::Home(".gemini/config/mcp_config.json")),
        instructions: Instructions::File(".agents/rules/tumr-runtime.md"),
        detect_commands: &[],
        detect_places: &[Place::Home(".gemini/antigravity")],
        setup: true,
        verified: "2026-11-19",
        notes: "No `type` key: `command` means a local stdio server, `serverUrl` a remote one. The user file applies to the App, the CLI and the IDE; the workspace file `.agents/mcp_config.json` to the CLI and the IDE. Rules in `rules/` need front matter with a `trigger`, or they are skipped.",
    },
];

#[must_use]
pub fn find(id: &str) -> Option<&'static Client> {
    CLIENTS.iter().find(|c| c.id == id)
}

/// Ids for error messages.
#[must_use]
pub fn ids(setup_only: bool) -> String {
    CLIENTS
        .iter()
        .filter(|c| !setup_only || c.setup)
        .map(|c| c.id)
        .collect::<Vec<_>>()
        .join(", ")
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> Option<PathBuf> {
    env_dir("USERPROFILE").or_else(|| env_dir("HOME"))
}

/// Claude Desktop's packaged (MSIX) build keeps its roaming data inside the package folder.
fn claude_desktop_package() -> Option<PathBuf> {
    let packages = env_dir("LOCALAPPDATA")?.join("Packages");
    std::fs::read_dir(packages)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("Claude_"))
        })
}

/// The folder holding Claude Desktop's `claude_desktop_config.json`.
///
/// Checked on a real install (Claude 5.11492, 2026-11-14): the packaged app keeps its settings in
/// `%LOCALAPPDATA%\Claude-Data\<package family, lower case>\` and leaves a
/// `.user-data-left.<package>` marker in the folders it no longer uses (`%APPDATA%\Claude` and the
/// MSIX package's `LocalCache\Roaming\Claude`). So: the first candidate that already has the
/// config file wins; otherwise the first existing one without such a marker.
#[must_use]
pub fn claude_desktop_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = env_dir("LOCALAPPDATA") {
        if let Ok(rd) = std::fs::read_dir(local.join("Claude-Data")) {
            candidates.extend(
                rd.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir()),
            );
        }
    }
    if let Some(pkg) = claude_desktop_package() {
        candidates.push(pkg.join("LocalCache").join("Roaming").join("Claude"));
    }
    if let Some(roaming) = env_dir("APPDATA") {
        candidates.push(roaming.join("Claude"));
    }
    let left = |d: &Path| {
        std::fs::read_dir(d).is_ok_and(|rd| {
            rd.filter_map(Result::ok).any(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with(".user-data-left."))
            })
        })
    };
    candidates
        .iter()
        .find(|d| d.join("claude_desktop_config.json").is_file())
        .or_else(|| candidates.iter().find(|d| d.is_dir() && !left(d)))
        .or_else(|| candidates.last())
        .cloned()
}

impl Place {
    /// The full path; `None` when the base folder is unknown or a project path has no project.
    #[must_use]
    pub fn resolve(self, project: Option<&Path>) -> Option<PathBuf> {
        // relative paths are written with `/`; join part by part so Windows paths stay uniform
        let join = |base: PathBuf, rel: &str| {
            rel.split('/')
                .filter(|s| !s.is_empty())
                .fold(base, |b, s| b.join(s))
        };
        match self {
            Place::Project(rel) => project.map(|p| join(p.to_path_buf(), rel)),
            Place::Home(rel) => home().map(|h| join(h, rel)),
            Place::AppData(rel) => env_dir("APPDATA").map(|a| join(a, rel)),
            Place::CodexHome(rel) => env_dir("CODEX_HOME")
                .or_else(|| home().map(|h| h.join(".codex")))
                .map(|c| join(c, rel)),
            Place::ClaudeDesktop => {
                claude_desktop_dir().map(|d| d.join("claude_desktop_config.json"))
            }
        }
    }
}

fn on_path(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        ["", ".exe", ".cmd", ".bat", ".ps1"]
            .iter()
            .any(|ext| dir.join(format!("{command}{ext}")).is_file())
    })
}

impl Client {
    /// Installed on this PC, as far as its command or its folders show.
    #[must_use]
    pub fn installed(&self) -> bool {
        self.detect_commands.iter().any(|c| on_path(c))
            || self
                .detect_places
                .iter()
                .filter_map(|p| p.resolve(None))
                .any(|p| p.exists())
            || (self.id == "claude-desktop" && claude_desktop_package().is_some())
    }

    /// The MCP server entry this client gets for the runtime (JSON clients).
    #[must_use]
    pub fn server_entry(&self, exe: &str) -> Value {
        let mut v = json!({ "command": exe, "args": ["mcp", "--client", self.id] });
        if let Format::Json { typed: true, .. } = self.format {
            v["type"] = json!("stdio");
        }
        v
    }

    /// Whether the given config file already has a `tumr` server.
    #[must_use]
    pub fn configured_in(&self, path: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        match self.format {
            Format::Json { key, .. } => serde_json::from_str::<Value>(&text)
                .is_ok_and(|v| v.get(key).and_then(|s| s.get("tumr")).is_some()),
            Format::CodexToml => text.lines().any(|l| l.trim() == "[mcp_servers.tumr]"),
        }
    }
}

/// `tumr agent clients [--project <folder>] [--json]`.
pub fn list(project: &Path, json_out: bool) -> std::process::ExitCode {
    let exe =
        std::env::current_exe().map_or_else(|_| "tumr.exe".into(), |p| p.display().to_string());
    let rows: Vec<Value> = CLIENTS
        .iter()
        .map(|c| {
            let user = c.user_config.and_then(|p| p.resolve(None));
            let proj = c.project_config.and_then(|p| p.resolve(Some(project)));
            json!({
                "id": c.id,
                "name": c.name,
                "installed": c.installed(),
                "setup": if c.setup { format!("tumr agent setup {}", c.id) } else { "manual".to_string() },
                "user_config": user.as_ref().map(|p| p.display().to_string()),
                "user_connected": user.as_deref().is_some_and(|p| c.configured_in(p)),
                "project_config": proj.as_ref().map(|p| p.display().to_string()),
                "project_connected": proj.as_deref().is_some_and(|p| c.configured_in(p)),
                "server_entry": match c.format {
                    Format::Json { key, .. } => json!({ key: { "tumr": c.server_entry(&exe) } }),
                    Format::CodexToml => json!(format!(
                        "[mcp_servers.tumr]
command = '{exe}'
args = [\"mcp\", \"--client\", \"{}\"]",
                        c.id
                    )),
                },
                "instructions": match c.instructions {
                    Instructions::File(f) | Instructions::Section(f) => Some(f),
                    Instructions::McpOnly => None,
                },
                "notes": c.notes,
                "verified": c.verified,
            })
        })
        .collect();
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        );
        return std::process::ExitCode::SUCCESS;
    }
    println!(
        "Coding agents on this PC (project: {}):\n",
        project.display()
    );
    for (c, r) in CLIENTS.iter().zip(&rows) {
        let connected = r["user_connected"] == true || r["project_connected"] == true;
        println!(
            "  {:<22} {:<13} {:<11} {}",
            c.name,
            if r["installed"] == true {
                "installed"
            } else {
                "not found"
            },
            if connected { "connected" } else { "-" },
            if c.setup {
                format!("tumr agent setup {}", c.id)
            } else {
                "manual setup (see docs)".into()
            }
        );
    }
    println!("\nConfig files: tumr agent clients --json");
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_setup_matches_writers() {
        let mut all: Vec<_> = CLIENTS.iter().map(|c| c.id).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), CLIENTS.len());
        // agent_setup::plan writes every client
        assert_eq!(ids(true), ids(false));
    }

    #[test]
    fn server_entry_follows_the_format() {
        let exe = r"C:\Programs\tumr.exe";
        let cursor = find("cursor").unwrap().server_entry(exe);
        assert_eq!(cursor["type"], "stdio");
        assert_eq!(cursor["args"], json!(["mcp", "--client", "cursor"]));
        let desktop = find("claude-desktop").unwrap().server_entry(exe);
        assert!(desktop.get("type").is_none());
        assert_eq!(desktop["command"], exe);
    }

    #[test]
    fn configured_in_reads_each_format() {
        let dir = std::env::temp_dir().join(format!("tumr-clients-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json_file = dir.join("mcp.json");
        std::fs::write(
            &json_file,
            r#"{"servers":{"tumr":{"type":"stdio","command":"x"}}}"#,
        )
        .unwrap();
        assert!(find("vscode").unwrap().configured_in(&json_file));
        assert!(!find("cursor").unwrap().configured_in(&json_file)); // key is mcpServers
        let toml = dir.join("config.toml");
        std::fs::write(
            &toml,
            "[other]\na = 1\n\n[mcp_servers.tumr]\ncommand = 'x'\n",
        )
        .unwrap();
        assert!(find("codex").unwrap().configured_in(&toml));
        assert!(!find("codex")
            .unwrap()
            .configured_in(&dir.join("missing.toml")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_places_need_a_project() {
        let p = Place::Project(".cursor/mcp.json");
        assert!(p.resolve(None).is_none());
        assert_eq!(
            p.resolve(Some(Path::new(r"D:\app"))).unwrap(),
            Path::new(r"D:\app").join(".cursor").join("mcp.json")
        );
    }
}
