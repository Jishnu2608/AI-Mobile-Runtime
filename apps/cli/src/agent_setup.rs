//! Connecting coding agents to the runtime (Phase 6 MCP-004/005, Phase 10 CON-002…004).
//!
//! * `umr agent setup <client> [--scope project|user] [--remove] [--print]`: writes (or removes)
//!   the client's `umr` MCP server entry and its agent instructions, after showing exactly what
//!   changes and asking. Other servers and settings in those files are kept; a re-run changes
//!   nothing; `--remove` takes out only what setup added and revokes the client's token.
//! * `umr agent doctor [client] [--fix]`: checks every connection end to end, including a real
//!   MCP handshake with the program the client would start.
//!
//! The clients and their file formats come from [`crate::agent_clients`].

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::time::Duration;

use serde_json::{json, Map, Value};
use umr_api::methods;

use crate::agent_clients::{self, Client, Format, Place, CLIENTS};
use crate::agent_cmd::{call, ensure_client, find_client, AGENT_SCOPES};
use crate::core_process::CoreTarget;
use crate::fail;
use crate::mcp_cmd::token_path;

// ---- scopes ------------------------------------------------------------------------------------

/// Where the server entry goes: the project's config or the user's (all projects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Project,
    User,
}

impl Scope {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "project" => Ok(Self::Project),
            "user" => Ok(Self::User),
            other => Err(format!("unknown scope '{other}': project or user")),
        }
    }
    fn word(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

/// The scope used when none is given: what each client's setup wrote before scopes existed.
#[must_use]
pub fn default_scope(c: &Client) -> Scope {
    if c.project_config.is_some() && c.id != "codex" {
        Scope::Project
    } else {
        Scope::User
    }
}

fn config_place(c: &Client, scope: Scope) -> Result<Place, String> {
    match scope {
        Scope::Project => c.project_config,
        Scope::User => c.user_config,
    }
    .ok_or_else(|| format!("{} has no {}-scope configuration", c.name, scope.word()))
}

/// Where the agent instructions go for this client and scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstrTarget {
    /// A file that belongs to the runtime (created and removed whole).
    Own(Place),
    /// A marked section of a shared `AGENTS.md`.
    Section(Place),
}

fn instructions_target(c: &Client, scope: Scope) -> Option<InstrTarget> {
    match (c.id, scope) {
        ("claude-code", Scope::Project) => Some(InstrTarget::Own(Place::Project(
            ".claude/skills/umr-runtime/SKILL.md",
        ))),
        ("claude-code", Scope::User) => Some(InstrTarget::Own(Place::Home(
            ".claude/skills/umr-runtime/SKILL.md",
        ))),
        ("codex", _) => Some(InstrTarget::Section(Place::Project("AGENTS.md"))),
        ("cursor", Scope::Project) => Some(InstrTarget::Own(Place::Project(
            ".cursor/rules/umr-runtime.mdc",
        ))),
        ("vscode", Scope::Project) => Some(InstrTarget::Own(Place::Project(
            ".github/instructions/umr-runtime.instructions.md",
        ))),
        // Claude Desktop, and Cursor / VS Code at user scope, have no instructions file:
        // the agent gets the MCP `instructions` sent at start-up.
        _ => None,
    }
}

const SKILL_DESCRIPTION: &str = "Run and check the user's Android app (Kotlin/Java, Flutter, React Native/Expo) on the AI Mobile Runtime emulator through the `umr` MCP tools: status after edits, problems with file:line, screenshots, taps, reloads.";

/// The instructions file content for a client (CON-003).
fn instructions_text(c: &Client) -> String {
    let body = umr_mcp::instructions::skill_markdown(false);
    match c.id {
        "claude-code" => umr_mcp::instructions::skill_markdown(true),
        // Cursor project rule: .mdc with front matter; always loaded
        "cursor" => {
            format!("---\ndescription: {SKILL_DESCRIPTION}\nalwaysApply: true\n---\n\n{body}")
        }
        // VS Code instructions file: applies to every file in the workspace
        "vscode" => format!(
            "---\nname: umr-runtime\ndescription: {SKILL_DESCRIPTION}\napplyTo: '**'\n---\n\n{body}"
        ),
        _ => body,
    }
}

// ---- file edits ----------------------------------------------------------------------------------

/// A planned file change. `new: None` deletes the file.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// Lines that differ between `old` and `new` (common start and end left out), `-` / `+`.
#[must_use]
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let removed = a[pre..a.len() - suf].iter().map(|l| format!("- {l}\n"));
    let added = b[pre..b.len() - suf].iter().map(|l| format!("+ {l}\n"));
    removed.chain(added).collect()
}

fn parse_json(old: Option<&str>, path: &str) -> Result<Value, String> {
    let v: Value = match old {
        Some(t) if !t.trim().is_empty() => {
            // Claude Desktop and VS Code files may start with a BOM
            serde_json::from_str(t.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("{path} is not valid JSON ({e}); fix it first"))?
        }
        _ => json!({}),
    };
    if v.is_object() {
        Ok(v)
    } else {
        Err(format!("{path} is not a JSON object"))
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default() + "\n"
}

/// The JSON config with `<key>.umr` set to `entry`; everything else kept in its order.
pub fn json_set(old: Option<&str>, path: &str, key: &str, entry: &Value) -> Result<String, String> {
    let mut v = parse_json(old, path)?;
    if !v[key].is_object() {
        v[key] = json!({});
    }
    v[key]["umr"] = entry.clone();
    Ok(pretty(&v))
}

/// The JSON config without `<key>.umr`; `None` when nothing else is left (the file was ours).
pub fn json_remove(old: &str, path: &str, key: &str) -> Result<Option<String>, String> {
    let mut v = parse_json(Some(old), path)?;
    let Some(servers) = v.get_mut(key).and_then(Value::as_object_mut) else {
        return Ok(Some(old.to_string()));
    };
    servers.remove("umr");
    if servers.is_empty() {
        if let Some(o) = v.as_object_mut() {
            o.remove(key);
        }
    }
    Ok(if v.as_object().is_some_and(Map::is_empty) {
        None
    } else {
        Some(pretty(&v))
    })
}

/// Codex `config.toml` with the `[mcp_servers.umr]` table set (the rest kept as it is).
#[must_use]
pub fn codex_toml(old: Option<&str>, exe: &str, client: &str) -> String {
    let table = format!(
        "[mcp_servers.umr]\ncommand = '{exe}'\nargs = [\"mcp\", \"--client\", \"{client}\"]\nstartup_timeout_sec = 30\ntool_timeout_sec = 600\n"
    );
    let old = old.unwrap_or("");
    match codex_table_range(old) {
        Some((start, end, lines)) => {
            let mut out: Vec<String> = lines[..start].iter().map(ToString::to_string).collect();
            out.extend(table.lines().map(String::from));
            if end < lines.len() {
                out.push(String::new());
            }
            out.extend(lines[end..].iter().map(ToString::to_string));
            out.join("\n") + "\n"
        }
        None if old.trim().is_empty() => table,
        None => format!("{}\n\n{table}", old.trim_end()),
    }
}

/// Codex `config.toml` without the `[mcp_servers.umr]` table; `None` when nothing else is left.
#[must_use]
pub fn codex_toml_remove(old: &str) -> Option<String> {
    let Some((start, end, lines)) = codex_table_range(old) else {
        return Some(old.to_string());
    };
    let mut kept: Vec<&str> = lines[..start].to_vec();
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let rest = &lines[end..];
    if !kept.is_empty() && !rest.is_empty() {
        kept.push("");
    }
    kept.extend_from_slice(rest);
    let text = kept.join("\n");
    if text.trim().is_empty() {
        None
    } else {
        Some(text.trim_end().to_string() + "\n")
    }
}

/// Start and end line of the `[mcp_servers.umr]` table, and the lines.
fn codex_table_range(text: &str) -> Option<(usize, usize, Vec<&str>)> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.trim() == "[mcp_servers.umr]")?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |j| start + 1 + j);
    Some((start, end, lines))
}

const BEGIN: &str = "<!-- umr:begin (written by `umr agent setup`; replaced on the next setup) -->";
const END: &str = "<!-- umr:end -->";

/// `AGENTS.md` with the runtime section set (marked; the rest kept).
#[must_use]
pub fn agents_md(old: Option<&str>) -> String {
    let section = format!(
        "{BEGIN}\n{}{END}\n",
        umr_mcp::instructions::skill_markdown(false)
    );
    let old = old.unwrap_or("");
    match (old.find(BEGIN), old.find(END)) {
        (Some(a), Some(b)) if b > a => format!(
            "{}{section}{}",
            &old[..a],
            old[b + END.len()..].trim_start_matches('\n')
        ),
        _ if old.trim().is_empty() => section,
        _ => format!("{}\n\n{section}", old.trim_end()),
    }
}

/// `AGENTS.md` without the runtime section; `None` when nothing else is left.
#[must_use]
pub fn agents_md_remove(old: &str) -> Option<String> {
    match (old.find(BEGIN), old.find(END)) {
        (Some(a), Some(b)) if b > a => {
            let before = old[..a].trim_end();
            let after = old[b + END.len()..].trim_start_matches('\n');
            let text = match (before.is_empty(), after.trim().is_empty()) {
                (true, true) => String::new(),
                (true, false) => after.to_string(),
                (false, true) => format!("{before}\n"),
                (false, false) => format!("{before}\n\n{after}"),
            };
            (!text.trim().is_empty()).then_some(text)
        }
        _ => Some(old.to_string()),
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn resolve(place: Place, project: &Path) -> Result<PathBuf, String> {
    place
        .resolve(Some(project))
        .ok_or_else(|| "cannot find the home or AppData folder".into())
}

/// Everything `setup` (or `setup --remove`) would change for one client and scope.
pub fn plan(
    c: &Client,
    scope: Scope,
    project: &Path,
    exe: &str,
    remove: bool,
) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    let path = resolve(config_place(c, scope)?, project)?;
    let shown = path.display().to_string();
    let old = read(&path);
    let new = match (c.format, remove) {
        (Format::Json { key, .. }, false) => {
            Some(json_set(old.as_deref(), &shown, key, &c.server_entry(exe))?)
        }
        (Format::Json { key, .. }, true) => match &old {
            Some(o) => json_remove(o, &shown, key)?,
            None => None,
        },
        (Format::CodexToml, false) => Some(codex_toml(old.as_deref(), exe, c.id)),
        (Format::CodexToml, true) => old.as_deref().and_then(codex_toml_remove),
    };
    out.push(Change { path, old, new });

    match instructions_target(c, scope) {
        Some(InstrTarget::Own(place)) => {
            let path = resolve(place, project)?;
            let old = read(&path);
            let new = (!remove).then(|| instructions_text(c));
            out.push(Change { path, old, new });
        }
        Some(InstrTarget::Section(place)) => {
            let path = resolve(place, project)?;
            let old = read(&path);
            let new = if remove {
                old.as_deref().and_then(agents_md_remove)
            } else {
                Some(agents_md(old.as_deref()))
            };
            out.push(Change { path, old, new });
        }
        None => {}
    }
    // nothing to do: same text, or deleting a file that is not there
    out.retain(|ch| ch.old != ch.new);
    Ok(out)
}

/// Writes the changes. Deleting a file inside `project` also removes the folders that are left
/// empty, up to the project folder (setup created them); outside the project only our own
/// `umr-runtime` folder is removed.
fn apply(changes: &[Change], project: &Path) -> Result<(), String> {
    for c in changes {
        if let Some(text) = &c.new {
            if let Some(dir) = c.path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&c.path, text).map_err(|e| format!("{}: {e}", c.path.display()))?;
        } else {
            std::fs::remove_file(&c.path).map_err(|e| format!("{}: {e}", c.path.display()))?;
            let mut dir = c.path.parent();
            while let Some(d) = dir {
                let ours = (d.starts_with(project) && d != project)
                    || d.file_name().is_some_and(|n| n == "umr-runtime");
                let empty = std::fs::read_dir(d).is_ok_and(|mut rd| rd.next().is_none());
                if !ours || !empty || std::fs::remove_dir(d).is_err() {
                    break;
                }
                dir = d.parent();
            }
        }
    }
    Ok(())
}

fn show(changes: &[Change]) {
    for c in changes {
        let verb = match (&c.old, &c.new) {
            (None, _) => "create",
            (Some(_), Some(_)) => "change",
            (Some(_), None) => "delete",
        };
        println!("\n{verb} {}", c.path.display());
        print!(
            "{}",
            diff(
                c.old.as_deref().unwrap_or(""),
                c.new.as_deref().unwrap_or("")
            )
        );
    }
}

fn ask(question: &str) -> bool {
    print!("\n{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    matches!(answer.trim(), "y" | "Y" | "yes")
}

fn current_exe() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .map_err(|e| format!("cannot locate umr.exe: {e}"))
}

fn project_dir(project: Option<PathBuf>) -> Result<PathBuf, String> {
    let p = PathBuf::from(crate::session_cmd::absolute(
        &project.unwrap_or_else(|| PathBuf::from(".")),
    ));
    if p.is_dir() {
        Ok(p)
    } else {
        Err(format!("{} is not a folder", p.display()))
    }
}

/// The options of `umr agent setup` (one field per command-line flag).
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct SetupOptions {
    pub project: Option<PathBuf>,
    pub scope: Option<String>,
    pub yes: bool,
    pub dry_run: bool,
    pub remove: bool,
    pub print: bool,
    /// Machine-readable result (the desktop app): never asks; applies only with `yes`.
    pub json: bool,
}

/// `umr agent setup <client> …`
pub async fn setup(target: &CoreTarget, agent: &str, o: SetupOptions) -> ExitCode {
    let Some(c) = agent_clients::find(agent) else {
        return fail(&format!(
            "unknown agent '{agent}'; known: {}. Any other MCP client: `umr agent add <name>`, then a stdio server `umr.exe mcp --client <name>`",
            agent_clients::ids(false)
        ));
    };
    let exe = match current_exe() {
        Ok(e) => e,
        Err(e) => return fail(&e),
    };
    if o.print {
        print_entry(c, &exe);
        return ExitCode::SUCCESS;
    }
    let project = match project_dir(o.project) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let scope = match o
        .scope
        .as_deref()
        .map_or(Ok(default_scope(c)), Scope::parse)
    {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let changes = match plan(c, scope, &project, &exe, o.remove) {
        Ok(ch) => ch,
        Err(e) => return fail(&e),
    };
    if o.json {
        return match setup_json(
            target,
            c,
            scope,
            &project,
            &changes,
            o.remove,
            o.yes && !o.dry_run,
        )
        .await
        {
            Ok(v) => {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        };
    }
    if o.remove {
        return remove(target, c, &project, scope, &changes, o.yes, o.dry_run).await;
    }

    let has_client = token_path(target, c.id).is_file();
    println!(
        "Connecting {} to AI Mobile Runtime ({} scope):",
        c.name,
        scope.word()
    );
    if !has_client {
        println!(
            "  • create agent client '{}' (scopes: {}; new project folders and device switches ask you first)",
            c.id,
            AGENT_SCOPES.join(", ")
        );
    }
    if changes.is_empty() {
        println!("  • its files are already up to date");
    }
    show(&changes);
    if o.dry_run {
        println!("\n(dry run: nothing written)");
        return ExitCode::SUCCESS;
    }
    if !has_client || !changes.is_empty() {
        if !o.yes && !ask("Apply?") {
            println!("Nothing changed.");
            return ExitCode::SUCCESS;
        }
        if let Err(e) = ensure_client(target, c.id, &AGENT_SCOPES.map(String::from)).await {
            return fail(&e);
        }
        if let Err(e) = apply(&changes, &project) {
            return fail(&e);
        }
    }
    println!("\nDone. {}", next_step(c, scope, &project));
    println!("Check the connection any time: umr agent doctor {}", c.id);
    ExitCode::SUCCESS
}

/// `--json`: the plan (and, with `apply`, the result) for the desktop app's agent page.
async fn setup_json(
    target: &CoreTarget,
    c: &Client,
    scope: Scope,
    project: &Path,
    changes: &[Change],
    remove: bool,
    apply_now: bool,
) -> Result<Value, String> {
    let client_exists = token_path(target, c.id).is_file();
    let listed: Vec<Value> = changes
        .iter()
        .map(|ch| {
            let action = match (&ch.old, &ch.new) {
                (None, _) => "create",
                (Some(_), Some(_)) => "change",
                (Some(_), None) => "delete",
            };
            json!({
                "path": ch.path.display().to_string(),
                "action": action,
                "diff": diff(ch.old.as_deref().unwrap_or(""), ch.new.as_deref().unwrap_or("")),
            })
        })
        .collect();
    if apply_now {
        if remove {
            apply(changes, project)?;
            if let Ok(Some(client)) = find_client(target, c.id).await {
                call(
                    target,
                    methods::AUTH_CLIENT_REVOKE,
                    json!({ "id": client["id"] }),
                )
                .await?;
            }
            let _ = std::fs::remove_file(token_path(target, c.id));
        } else {
            ensure_client(target, c.id, &AGENT_SCOPES.map(String::from)).await?;
            apply(changes, project)?;
        }
    }
    Ok(json!({
        "client": c.id,
        "name": c.name,
        "scope": scope.word(),
        "project": project.display().to_string(),
        "remove": remove,
        "client_exists": client_exists,
        "changes": listed,
        "applied": apply_now,
        "next_step": if remove {
            format!("{} is disconnected; restart it if it is open.", c.name)
        } else {
            next_step(c, scope, project)
        },
    }))
}

// ---- install links (CON-006) ---------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Percent-encodes everything except unreserved URL characters.
fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A one-click install link for clients that have one. It carries this PC's `umr.exe` path, so
/// it is made on the PC (CLI or desktop app), never on the website (DEC-055).
pub fn install_link(c: &Client, exe: &str) -> Option<String> {
    let entry = c.server_entry(exe);
    match c.id {
        // https://cursor.com/docs/mcp/install-links: base64 of the server config, then URL-encoded
        "cursor" => Some(format!(
            "cursor://anysphere.cursor-deeplink/mcp/install?name=umr&config={}",
            url_encode(&base64(entry.to_string().as_bytes()))
        )),
        // https://code.visualstudio.com/api/extension-guides/ai/mcp: URL-encoded JSON with a name
        "vscode" => {
            let mut v = entry;
            v["name"] = json!("umr");
            Some(format!("vscode:mcp/install?{}", url_encode(&v.to_string())))
        }
        _ => None,
    }
}

/// `umr agent link <cursor|vscode> [--json]`: creates the agent client if needed and prints the
/// install link; opening it lets the client add the `umr` server itself (it asks first).
pub async fn link(target: &CoreTarget, agent: &str, json_out: bool) -> ExitCode {
    let Some(c) = agent_clients::find(agent) else {
        return fail(&format!(
            "unknown agent '{agent}'; known: {}",
            agent_clients::ids(false)
        ));
    };
    let exe = match current_exe() {
        Ok(e) => e,
        Err(e) => return fail(&e),
    };
    let Some(url) = install_link(c, &exe) else {
        return fail(&format!(
            "{} has no install link; use `umr agent setup {}`",
            c.name, c.id
        ));
    };
    if let Err(e) = ensure_client(target, c.id, &AGENT_SCOPES.map(String::from)).await {
        return fail(&e);
    }
    if json_out {
        println!("{}", json!({ "client": c.id, "link": url }));
    } else {
        println!("Install link for {} (open it on this PC; {} shows the server and asks before adding it):\n\n{url}\n", c.name, c.name);
        println!("The link only adds the MCP server. For the agent instructions as well, use: umr agent setup {}", c.id);
    }
    ExitCode::SUCCESS
}

fn next_step(c: &Client, scope: Scope, project: &Path) -> String {
    let p = project.display();
    match (c.id, scope) {
        ("claude-code", Scope::Project) => format!("Start Claude Code in {p} and approve the project MCP server `umr` when it asks (`/mcp` shows it)."),
        ("claude-code", Scope::User) => "Start Claude Code in any project; `/mcp` lists the `umr` server.".into(),
        ("codex", _) => format!("Start Codex in {p}; `/mcp` lists the `umr` server."),
        ("cursor", _) => "Open Cursor Settings → MCP and make sure `umr` is switched on (Cursor may ask to enable a new server).".into(),
        ("claude-desktop", _) => "Quit Claude Desktop completely (also from the tray) and start it again; the `umr` tools appear under the tools menu.".into(),
        ("vscode", _) => "In VS Code, open the Chat view in agent mode; start the `umr` server when VS Code offers it (MCP: List Servers).".into(),
        _ => String::new(),
    }
}

async fn remove(
    target: &CoreTarget,
    c: &Client,
    project: &Path,
    scope: Scope,
    changes: &[Change],
    yes: bool,
    dry_run: bool,
) -> ExitCode {
    let has_client = matches!(find_client(target, c.id).await, Ok(Some(_)))
        || token_path(target, c.id).is_file();
    println!(
        "Disconnecting {} from AI Mobile Runtime ({} scope):",
        c.name,
        scope.word()
    );
    if changes.is_empty() {
        println!("  • nothing of the runtime is in its files");
    }
    if has_client {
        println!(
            "  • remove agent client '{}' (its token stops working)",
            c.id
        );
    }
    show(changes);
    if dry_run {
        println!("\n(dry run: nothing changed)");
        return ExitCode::SUCCESS;
    }
    if changes.is_empty() && !has_client {
        return ExitCode::SUCCESS;
    }
    if !yes && !ask("Remove?") {
        println!("Nothing changed.");
        return ExitCode::SUCCESS;
    }
    if let Err(e) = apply(changes, project) {
        return fail(&e);
    }
    if let Ok(Some(client)) = find_client(target, c.id).await {
        if let Err(e) = call(
            target,
            methods::AUTH_CLIENT_REVOKE,
            json!({ "id": client["id"] }),
        )
        .await
        {
            return fail(&e);
        }
    }
    let _ = std::fs::remove_file(token_path(target, c.id));
    println!(
        "\nDone. {} is disconnected; restart it if it is open.",
        c.name
    );
    ExitCode::SUCCESS
}

/// `--print`: the entry to add by hand, for clients set up some other way.
fn print_entry(c: &Client, exe: &str) {
    println!(
        "MCP server entry for {} (stdio, no secret in it):\n",
        c.name
    );
    match c.format {
        Format::Json { key, .. } => println!(
            "{}",
            serde_json::to_string_pretty(&json!({ key: { "umr": c.server_entry(exe) } }))
                .unwrap_or_default()
        ),
        Format::CodexToml => print!("{}", codex_toml(None, exe, c.id)),
    }
    println!(
        "\nThe client '{}' needs its token first: umr agent add {}",
        c.id, c.id
    );
}

// ---- doctor (CON-004) ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Ok,
    Warn,
    Fail,
}

impl Mark {
    fn word(self) -> &'static str {
        match self {
            Self::Ok => "ok  ",
            Self::Warn => "warn",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug)]
struct Check {
    mark: Mark,
    what: String,
    fix: Option<String>,
}

fn check(mark: Mark, what: impl Into<String>, fix: Option<String>) -> Check {
    Check {
        mark,
        what: what.into(),
        fix,
    }
}

/// The `umr` server entry in a config file: (command, args).
fn entry_in(c: &Client, path: &Path) -> Option<(String, Vec<String>)> {
    let text = read(path)?;
    match c.format {
        Format::Json { key, .. } => {
            let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
            let e = v.get(key)?.get("umr")?;
            let args = e["args"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a.as_str().map(String::from))
                .collect();
            Some((e["command"].as_str()?.to_string(), args))
        }
        Format::CodexToml => {
            let (start, end, lines) = codex_table_range(&text)?;
            let mut command = None;
            let mut args = Vec::new();
            for l in &lines[start + 1..end] {
                let l = l.trim();
                if let Some(v) = l.strip_prefix("command") {
                    let v = v.trim_start().trim_start_matches('=').trim();
                    command = Some(v.trim_matches(|ch| ch == '\'' || ch == '"').to_string());
                } else if let Some(v) = l.strip_prefix("args") {
                    let v = v.trim_start().trim_start_matches('=').trim();
                    args = v
                        .trim_matches(|ch| ch == '[' || ch == ']')
                        .split(',')
                        .map(|a| a.trim().trim_matches('"').trim_matches('\'').to_string())
                        .filter(|a| !a.is_empty())
                        .collect();
                }
            }
            Some((command?, args))
        }
    }
}

/// Why a config file that exists cannot be used, if it cannot.
fn config_problem(c: &Client, path: &Path) -> Option<String> {
    let text = read(path)?;
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return None; // an empty file is a config without servers
    }
    match c.format {
        Format::Json { .. } => serde_json::from_str::<Value>(text).err().map(|e| {
            format!(
                "{} is not plain JSON ({e}); comments and trailing commas are not supported, edit it by hand",
                path.display()
            )
        }),
        Format::CodexToml => None,
    }
}

fn client_name_in(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--client")
        .and_then(|i| args.get(i + 1).cloned())
}

/// Starts `command args…` the way the agent would and runs `initialize` + `tools/list`.
/// Returns the number of tools.
fn handshake(command: &str, args: &[String], timeout: Duration) -> Result<usize, String> {
    let mut cmd = std::process::Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // no console window flashing up when the desktop app runs the doctor
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot start it: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    let err_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let err_sink = err_text.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut s) = err_sink.lock() {
                s.push_str(&line);
                s.push('\n');
            }
        }
    });
    let send = |stdin: &mut std::process::ChildStdin, v: &Value| -> Result<(), String> {
        writeln!(stdin, "{v}")
            .and_then(|()| stdin.flush())
            .map_err(|e| e.to_string())
    };
    let result = (|| {
        send(
            &mut stdin,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": { "name": "umr-agent-doctor", "version": env!("CARGO_PKG_VERSION") } } }),
        )?;
        let deadline = std::time::Instant::now() + timeout;
        let wait = |id: i64| -> Result<Value, String> {
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                let v = rx
                    .recv_timeout(left)
                    .map_err(|_| "no answer in time".to_string())?;
                if v["id"] == id {
                    return match v.get("error") {
                        Some(e) => Err(e["message"].as_str().unwrap_or("error").to_string()),
                        None => Ok(v["result"].clone()),
                    };
                }
            }
        };
        wait(1)?;
        send(
            &mut stdin,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )?;
        send(
            &mut stdin,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        )?;
        let tools = wait(2)?;
        Ok(tools["tools"].as_array().map_or(0, Vec::len))
    })();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    result.map_err(|e| {
        let stderr = err_text
            .lock()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if stderr.is_empty() {
            e
        } else {
            format!("{e} ({})", stderr.lines().last().unwrap_or_default())
        }
    })
}

/// Checks for one client. `None` when it is not connected anywhere and was not asked for.
async fn doctor_client(
    target: &CoreTarget,
    c: &Client,
    project: &Path,
    exe: &str,
    asked: bool,
    core_up: bool,
) -> Option<(Vec<Check>, Vec<Scope>)> {
    let mut found = Vec::new();
    let mut broken = Vec::new();
    for scope in [Scope::Project, Scope::User] {
        let Ok(place) = config_place(c, scope) else {
            continue;
        };
        let Ok(path) = resolve(place, project) else {
            continue;
        };
        if let Some(problem) = config_problem(c, &path) {
            broken.push(problem);
        } else if let Some(entry) = entry_in(c, &path) {
            found.push((scope, path, entry));
        }
    }
    let mut checks: Vec<Check> = broken
        .into_iter()
        .map(|p| {
            check(
                Mark::Fail,
                p,
                Some("fix the file by hand, then run setup again".into()),
            )
        })
        .collect();
    if found.is_empty() && !checks.is_empty() {
        return Some((checks, Vec::new()));
    }
    if found.is_empty() {
        if !asked {
            return None;
        }
        checks.push(check(
            Mark::Fail,
            format!(
                "no `umr` server in its config ({})",
                if c.installed() {
                    "installed"
                } else {
                    "not found on this PC"
                }
            ),
            Some(format!("umr agent setup {}", c.id)),
        ));
        return Some((checks, Vec::new()));
    }
    let mut stale = Vec::new();
    for (scope, path, (command, args)) in &found {
        let (more, is_stale) =
            check_entry(target, c, *scope, path, command, args, exe, core_up).await;
        checks.extend(more);
        if is_stale {
            stale.push(*scope);
        }
    }
    Some((checks, stale))
}

/// Checks for one `umr` entry: program path, token, last call, MCP handshake.
/// The flag says whether the entry's program path should be rewritten (`--fix`).
#[allow(clippy::too_many_arguments)]
async fn check_entry(
    target: &CoreTarget,
    c: &Client,
    scope: Scope,
    path: &Path,
    command: &str,
    args: &[String],
    exe: &str,
    core_up: bool,
) -> (Vec<Check>, bool) {
    let mut checks = vec![check(
        Mark::Ok,
        format!("{} scope: {}", scope.word(), path.display()),
        None,
    )];
    if !Path::new(command).is_file() {
        checks.push(check(
            Mark::Fail,
            format!("program not found: {command}"),
            Some(format!("umr agent doctor {} --fix", c.id)),
        ));
        return (checks, true);
    }
    let same = Path::new(command)
        .canonicalize()
        .ok()
        .zip(Path::new(exe).canonicalize().ok())
        .is_some_and(|(a, b)| a == b);
    if !same {
        checks.push(check(
            Mark::Warn,
            format!("points at another umr.exe: {command}"),
            Some(format!("umr agent doctor {} --fix", c.id)),
        ));
    }
    let Some(name) = client_name_in(args) else {
        checks.push(check(
            Mark::Fail,
            "entry has no `--client <name>` argument",
            Some(format!("umr agent setup {} --scope {}", c.id, scope.word())),
        ));
        return (checks, !same);
    };
    if token_path(target, &name).is_file() {
        checks.push(check(Mark::Ok, format!("token for client '{name}'"), None));
    } else {
        checks.push(check(
            Mark::Fail,
            format!("no token for client '{name}'"),
            Some(format!("umr agent add {name}")),
        ));
        return (checks, !same);
    }
    if core_up {
        // read before the handshake, which logs in itself
        checks.push(match last_call(target, &name).await {
            Some(t) => check(Mark::Ok, format!("last call {t}"), None),
            None => check(Mark::Ok, "no tool calls from this agent yet", None),
        });
    }
    match handshake(command, args, Duration::from_secs(30)) {
        Ok(n) => checks.push(check(Mark::Ok, format!("MCP handshake: {n} tools"), None)),
        Err(e) => checks.push(check(
            Mark::Fail,
            format!("MCP handshake failed: {e}"),
            Some("umr status; umr start".into()),
        )),
    }
    (checks, !same)
}

async fn last_call(target: &CoreTarget, name: &str) -> Option<String> {
    let id = find_client(target, name).await.ok()??["id"]
        .as_str()?
        .to_string();
    let v = call(
        target,
        methods::AUDIT_LIST,
        json!({ "client": id, "limit": 50 }),
    )
    .await
    .ok()?;
    // a log-in is only the connection (doctor's own handshake makes one too): skip it
    let at = v["entries"]
        .as_array()?
        .iter()
        .find(|e| e["action"] != "auth.login")?["at_unix"]
        .as_i64()?;
    let ago = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs(),
    )
    .ok()?
        - at;
    Some(match ago {
        a if a < 120 => format!("{a} s ago"),
        a if a < 7200 => format!("{} min ago", a / 60),
        a if a < 172_800 => format!("{} h ago", a / 3600),
        a => format!("{} days ago", a / 86400),
    })
}

/// `umr agent doctor [client] [--project] [--fix] [--json]`.
pub async fn doctor(
    target: &CoreTarget,
    agent: Option<String>,
    project: Option<PathBuf>,
    fix: bool,
    json_out: bool,
) -> ExitCode {
    let clients: Vec<&Client> = match &agent {
        Some(a) => match agent_clients::find(a) {
            Some(c) => vec![c],
            None => {
                return fail(&format!(
                    "unknown agent '{a}'; known: {}",
                    agent_clients::ids(false)
                ))
            }
        },
        None => CLIENTS.iter().collect(),
    };
    let project = match project_dir(project) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let exe = match current_exe() {
        Ok(e) => e,
        Err(e) => return fail(&e),
    };
    let core_up = matches!(target.connect().await, Ok(Some(_)));
    let mut report = Vec::new();
    let mut failed = false;
    if !json_out {
        println!(
            "Runtime Core: {}",
            if core_up {
                "running"
            } else {
                "not running (each agent starts it when it connects)"
            }
        );
    }
    for c in clients {
        let Some((checks, stale)) =
            doctor_client(target, c, &project, &exe, agent.is_some(), core_up).await
        else {
            continue;
        };
        let mut fixed = Vec::new();
        if fix {
            for scope in &stale {
                match plan(c, *scope, &project, &exe, false).and_then(|ch| apply(&ch, &project)) {
                    Ok(()) => fixed.push(scope.word()),
                    Err(e) => return fail(&e),
                }
            }
        }
        failed |= checks.iter().any(|k| k.mark == Mark::Fail) && fixed.is_empty();
        if json_out {
            report.push(json!({
                "client": c.id,
                "checks": checks.iter().map(|k| json!({ "result": format!("{:?}", k.mark).to_lowercase(), "what": k.what, "fix": k.fix })).collect::<Vec<_>>(),
                "fixed": fixed,
            }));
            continue;
        }
        println!("\n{}", c.name);
        for k in &checks {
            println!("  [{}] {}", k.mark.word(), k.what);
            if let (Some(f), Mark::Warn | Mark::Fail) = (&k.fix, k.mark) {
                println!("         fix: {f}");
            }
        }
        for s in &fixed {
            println!("  fixed: {s}-scope entry now points at {exe}");
        }
    }
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else if report.is_empty()
        && agent.is_none()
        && !CLIENTS.iter().any(|c| {
            [Scope::Project, Scope::User].iter().any(|s| {
                config_place(c, *s)
                    .ok()
                    .and_then(|p| resolve(p, &project).ok())
                    .is_some_and(|p| entry_in(c, &p).is_some())
            })
        })
    {
        println!("\nNo agent is connected yet. See the agents on this PC: umr agent clients");
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("umr-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn json_set_keeps_other_servers_and_key_order() {
        let old = r#"{"zeta": 1, "mcpServers": {"other": {"command": "x"}}, "alpha": 2}"#;
        let new = json_set(Some(old), "f", "mcpServers", &json!({"command": "umr"})).unwrap();
        let v: Value = serde_json::from_str(&new).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["umr"]["command"], "umr");
        assert!(
            new.find("zeta").unwrap() < new.find("alpha").unwrap(),
            "order kept"
        );
        assert!(json_set(Some("not json"), "f", "k", &json!({})).is_err());
        // a BOM is accepted
        assert!(json_set(Some("\u{feff}{}"), "f", "servers", &json!({})).is_ok());
    }

    #[test]
    fn json_remove_takes_out_only_ours() {
        let set = json_set(None, "f", "servers", &json!({"command": "umr"})).unwrap();
        assert_eq!(
            json_remove(&set, "f", "servers").unwrap(),
            None,
            "file was ours"
        );
        let shared = r#"{"servers": {"umr": {}, "other": {}}, "inputs": []}"#;
        let left: Value =
            serde_json::from_str(&json_remove(shared, "f", "servers").unwrap().unwrap()).unwrap();
        assert!(left["servers"].get("umr").is_none());
        assert!(left["servers"].get("other").is_some());
        let only_ours = r#"{"keep": true, "mcpServers": {"umr": {}}}"#;
        let left: Value =
            serde_json::from_str(&json_remove(only_ours, "f", "mcpServers").unwrap().unwrap())
                .unwrap();
        assert_eq!(left, json!({"keep": true}));
    }

    #[test]
    fn codex_table_is_added_replaced_and_removed() {
        let fresh = codex_toml(None, r"C:\u\umr.exe", "codex");
        assert!(fresh.starts_with("[mcp_servers.umr]\ncommand = 'C:\\u\\umr.exe'\n"));
        let old = "model = \"o3\"\n\n[mcp_servers.umr]\ncommand = 'old'\nargs = []\n\n[mcp_servers.other]\ncommand = 'x'\n";
        let new = codex_toml(Some(old), "new", "codex");
        assert!(new.starts_with("model = \"o3\"\n\n[mcp_servers.umr]\ncommand = 'new'\n"));
        assert!(new.contains("[mcp_servers.other]\ncommand = 'x'"));
        assert_eq!(new.matches("[mcp_servers.umr]").count(), 1);
        assert_eq!(codex_toml(Some(&new), "new", "codex"), new, "idempotent");
        let removed = codex_toml_remove(&new).unwrap();
        assert_eq!(
            removed,
            "model = \"o3\"\n\n[mcp_servers.other]\ncommand = 'x'\n"
        );
        let before = "model = \"o3\"\n";
        assert_eq!(
            codex_toml_remove(&codex_toml(Some(before), "e", "codex")).unwrap(),
            before,
            "round trip"
        );
        assert_eq!(codex_toml_remove(&fresh), None);
    }

    #[test]
    fn agents_section_round_trip() {
        let before = "# My project\nRules.\n";
        let once = agents_md(Some(before));
        assert!(once.starts_with("# My project\nRules.\n\n<!-- umr:begin"));
        assert_eq!(agents_md(Some(&once)), once, "idempotent");
        assert_eq!(agents_md_remove(&once).unwrap(), before);
        assert_eq!(agents_md_remove(&agents_md(None)), None);
    }

    #[test]
    fn diff_shows_only_changed_lines() {
        assert_eq!(diff("a\nb\nc\n", "a\nx\nc\n"), "- b\n+ x\n");
        assert_eq!(diff("", "new\n"), "+ new\n");
    }

    #[test]
    fn every_client_sets_up_and_removes_cleanly_in_a_project() {
        for id in ["claude-code", "cursor", "vscode"] {
            let dir = temp(id);
            let c = agent_clients::find(id).unwrap();
            let plan1 = plan(c, Scope::Project, &dir, r"C:\umr\umr.exe", false).unwrap();
            assert_eq!(plan1.len(), 2, "{id}: config + instructions");
            apply(&plan1, &dir).unwrap();
            let cfg = &plan1[0].path;
            assert_eq!(entry_in(c, cfg).unwrap().1, vec!["mcp", "--client", id]);
            assert!(
                plan(c, Scope::Project, &dir, r"C:\umr\umr.exe", false)
                    .unwrap()
                    .is_empty(),
                "{id}: idempotent"
            );
            let undo = plan(c, Scope::Project, &dir, r"C:\umr\umr.exe", true).unwrap();
            apply(&undo, &dir).unwrap();
            assert!(
                !cfg.exists() && !plan1[1].path.exists(),
                "{id}: files removed"
            );
            assert_eq!(
                std::fs::read_dir(&dir).unwrap().count(),
                0,
                "{id}: no empty folders left"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn remove_restores_a_shared_config() {
        let dir = temp("shared");
        let cfg = dir.join(".cursor").join("mcp.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let before =
            "{\n  \"mcpServers\": {\n    \"other\": {\n      \"command\": \"x\"\n    }\n  }\n}\n";
        std::fs::write(&cfg, before).unwrap();
        let c = agent_clients::find("cursor").unwrap();
        apply(
            &plan(c, Scope::Project, &dir, "umr.exe", false).unwrap(),
            &dir,
        )
        .unwrap();
        apply(
            &plan(c, Scope::Project, &dir, "umr.exe", true).unwrap(),
            &dir,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn instruction_files_have_their_client_header() {
        let cursor = instructions_text(agent_clients::find("cursor").unwrap());
        assert!(
            cursor.starts_with("---\ndescription: ") && cursor.contains("alwaysApply: true\n---\n")
        );
        let vscode = instructions_text(agent_clients::find("vscode").unwrap());
        assert!(vscode.contains("applyTo: '**'"));
        assert!(
            instructions_target(agent_clients::find("claude-desktop").unwrap(), Scope::User)
                .is_none()
        );
    }

    #[test]
    fn scopes_default_to_what_setup_always_wrote() {
        assert_eq!(
            default_scope(agent_clients::find("claude-code").unwrap()),
            Scope::Project
        );
        assert_eq!(
            default_scope(agent_clients::find("codex").unwrap()),
            Scope::User
        );
        assert_eq!(
            default_scope(agent_clients::find("claude-desktop").unwrap()),
            Scope::User
        );
        assert!(config_place(
            agent_clients::find("claude-desktop").unwrap(),
            Scope::Project
        )
        .is_err());
        assert!(Scope::parse("global").is_err());
    }

    #[test]
    fn codex_entry_is_read_back() {
        let dir = temp("codex-read");
        let p = dir.join("config.toml");
        std::fs::write(
            &p,
            codex_toml(Some("model = \"x\"\n"), r"C:\a b\umr.exe", "codex"),
        )
        .unwrap();
        let (cmd, args) = entry_in(agent_clients::find("codex").unwrap(), &p).unwrap();
        assert_eq!(cmd, r"C:\a b\umr.exe");
        assert_eq!(client_name_in(&args).as_deref(), Some("codex"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_configs_are_fine_and_comments_are_reported() {
        let dir = temp("problems");
        let c = agent_clients::find("vscode").unwrap();
        let p = dir.join("mcp.json");
        std::fs::write(&p, "").unwrap();
        assert_eq!(config_problem(c, &p), None);
        std::fs::write(
            &p,
            "{ // note
 \"servers\": {} }",
        )
        .unwrap();
        assert!(config_problem(c, &p).unwrap().contains("comments"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_links_encode_the_entry() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(url_encode("a b/=+"), "a%20b%2F%3D%2B");
        let exe = r"C:\Users\me\umr.exe";
        let cursor = install_link(agent_clients::find("cursor").unwrap(), exe).unwrap();
        assert!(
            cursor.starts_with("cursor://anysphere.cursor-deeplink/mcp/install?name=umr&config=")
        );
        let vscode = install_link(agent_clients::find("vscode").unwrap(), exe).unwrap();
        assert!(vscode.starts_with("vscode:mcp/install?%7B"));
        assert!(vscode.contains("%22name%22%3A%22umr%22"));
        assert!(install_link(agent_clients::find("codex").unwrap(), exe).is_none());
    }

    #[test]
    fn handshake_reports_a_program_that_cannot_start() {
        let e = handshake(r"C:\nowhere\umr.exe", &[], Duration::from_secs(1)).unwrap_err();
        assert!(e.starts_with("cannot start it"), "{e}");
    }
}
