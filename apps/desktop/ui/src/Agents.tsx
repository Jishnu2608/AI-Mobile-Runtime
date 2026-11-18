// "Connect your agent" (Phase 10.5, CON-005): the wizard's last step. Every action runs
// `umr agent … --json` through the desktop app (agents.rs), so the page shows exactly what the
// CLI would do: the files it changes, as a diff, before anything is written.
import { useEffect, useState } from "react";
import { Why } from "./why";
import { agentCli } from "./api";

type ClientRow = {
  id: string;
  name: string;
  installed: boolean;
  user_connected: boolean;
  project_connected: boolean;
  user_config: string | null;
  project_config: string | null;
};
type Plan = { name: string; scope: string; remove: boolean; client_exists: boolean; changes: { path: string; action: string; diff: string }[]; applied: boolean; next_step: string };
type Check = { result: "ok" | "warn" | "fail"; what: string; fix: string | null };
type Report = { client: string; checks: Check[]; fixed: string[] }[];

const DEFAULT_SCOPE: Record<string, "project" | "user"> = { "claude-code": "project", codex: "user", cursor: "project", "claude-desktop": "user", vscode: "project", antigravity: "user" };

/** Agents whose connection also installs the stop hook: the automatic error loop (HOOK-001). */
const LOOP: Record<string, true> = { "claude-code": true, codex: true, cursor: true, antigravity: true };

/** AUD-P1-8: whether the automatic error loop is on for this agent, in one sentence. */
function loopLine(row: ClientRow, connected: boolean): { ok: boolean; text: string } {
  if (!LOOP[row.id]) {
    return { ok: true, text: `No automatic loop for ${row.name}: ask it to check the app's problems (its runtime tools include them).` };
  }
  // AUD-P1-32: Claude Code also hears after each file edit; the others only when they finish
  const during = row.id === "claude-code" ? " It also hears about problems the app already shows after each file edit, while it works." : "";
  return connected
    ? { ok: true, text: `Automatic error loop on: when ${row.name} finishes a task, it is told about build errors, crashes and runtime errors in the running app and keeps fixing them.${during}` }
    : { ok: false, text: `Automatic error loop off: ${row.name} is not told about errors in the running app until you connect it.` };
}

/** The last folder name of a path. */
function folderName(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() ?? "";
}

/** `path` relative to `root` when it is inside it (for a .gitignore line). */
function relative(path: string, root: string): string {
  const norm = (x: string) => x.replace(/\\/g, "/").replace(/\/+$/, "");
  const p = norm(path);
  const r = norm(root);
  return p.toLowerCase().startsWith(r.toLowerCase() + "/") ? p.slice(r.length + 1) : p;
}

/** AUD-P2-28: a file inside the project that would carry this PC's umr.exe path into git.
 * Claude Code keeps settings.local.json out of git itself. */
function sharedWithPath(c: { path: string; diff: string }, project: string): boolean {
  if (!project) return false;
  const inside = relative(c.path, project) !== c.path.replace(/\\/g, "/").replace(/\/+$/, "");
  return inside && /umr\.exe/i.test(c.diff) && !/settings\.local\.json$/i.test(c.path);
}

export function AgentsStep({ project, onBack }: { project: string; onBack: () => void }) {
  const [rows, setRows] = useState<ClientRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => {
    setError(null);
    agentCli<ClientRow[]>({ action: "clients", project })
      .then(setRows)
      .catch((e) => setError(String(e)));
  };
  useEffect(load, [project]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <section>
      <h2>Connect your coding agent</h2>
      <Why step="agents" />
      <p>
        Your agent starts the runtime's MCP server on this PC and gets the screen, the problems and the controls. Connecting writes
        one entry into the agent's own settings; you see every change before it is made, and <strong>Disconnect</strong> takes it out
        again.
      </p>
      <p>
        <strong>The automatic error loop</strong> (Claude Code, Codex, Cursor, Antigravity): when your agent finishes a task, the runtime checks the
        app it changed. If the build failed, the app crashed or showed an error, the agent gets the problem with its file and line and
        keeps working, without you copying anything. It works when the agent is opened in the project folder, a folder above it (a
        repository root) or a folder inside it.
      </p>
      <p className="muted small">
        Project: <code>{project || "(none chosen)"}</code>
      </p>
      {rows && rows.some((r) => LOOP[r.id] && r.installed) && !rows.some((r) => LOOP[r.id] && (r.project_connected || r.user_connected)) && (
        <div className="problem" role="status">
          <strong>The automatic error loop is off.</strong> None of your coding agents is connected, so errors in the running app reach them
          only when you paste them in. Connect one below.
        </div>
      )}
      {error && <div className="problem" role="alert">{error} <button className="secondary" onClick={load}>Try again</button></div>}
      {!rows && !error && <p className="busy">Looking for coding agents on this PC…</p>}
      {rows && rows.map((r) => <AgentCard key={r.id} row={r} project={project} onChanged={load} />)}
      <p className="muted small">
        Other MCP clients: create a client with <code>umr agent add &lt;name&gt;</code> and start <code>umr.exe mcp --client &lt;name&gt;</code>. The
        claude.ai website and mobile app cannot reach a program on your PC.
      </p>
      <div className="nav">
        <button className="secondary" onClick={onBack}>Back</button>
        <span />
      </div>
    </section>
  );
}

function AgentCard({ row, project, onChanged }: { row: ClientRow; project: string; onChanged: () => void }) {
  const scopes = (["project", "user"] as const).filter((s) => (s === "project" ? row.project_config : row.user_config));
  const [scope, setScope] = useState<"project" | "user">(scopes.includes(DEFAULT_SCOPE[row.id]) ? DEFAULT_SCOPE[row.id] : scopes[0]);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const connected = row.project_connected || row.user_connected;
  const needsProject = scope === "project" && !project;

  const run = async <T,>(label: string, f: () => Promise<T>, then: (v: T) => void) => {
    setBusy(label);
    setError(null);
    try {
      then(await f());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };
  const preview = (remove: boolean) =>
    run("Preparing", () => agentCli<Plan>({ action: "plan", client: row.id, project, scope, remove }), (p) => {
      setReport(null);
      setDone(null);
      setPlan(p);
    });
  const apply = () =>
    run(plan?.remove ? "Disconnecting" : "Connecting", () => agentCli<Plan>({ action: "apply", client: row.id, project, scope, remove: plan?.remove }), (p) => {
      setPlan(null);
      setDone(p.next_step);
      onChanged();
    });
  const doctor = (fix: boolean) =>
    run(fix ? "Repairing" : "Testing the connection", () => agentCli<Report>({ action: fix ? "fix" : "doctor", client: row.id, project }), (r) => {
      setPlan(null);
      setReport(r);
      if (fix) onChanged();
    });

  return (
    <div className="card agent">
      <div className="row">
        <div>
          <p className="big">{row.name}</p>
          <p className="small">
            <span className={`tag ${row.installed ? "reuse" : ""}`}>{row.installed ? "Installed" : "Not found on this PC"}</span>{" "}
            <span className={`tag ${connected ? "install" : ""}`}>
              {connected ? `Connected (${[row.project_connected && "this project", row.user_connected && "all projects"].filter(Boolean).join(", ")})` : "Not connected"}
            </span>
          </p>
        </div>
        <div className="agent-actions">
          {scopes.length > 1 && (
            <select aria-label={`Where to connect ${row.name}`} value={scope} onChange={(e) => { setScope(e.target.value as "project" | "user"); setPlan(null); }}>
              <option value="project">This project</option>
              <option value="user">All projects</option>
            </select>
          )}
          <button onClick={() => preview(false)} disabled={!!busy || needsProject}>
            {connected ? "Update" : "Connect"}
          </button>
          {connected && (
            <button className="secondary" onClick={() => doctor(false)} disabled={!!busy} title="Starts the runtime's MCP server as this agent does, checks its settings, token and stop hook, and asks the runtime what the hook would report now">
              Test connection
            </button>
          )}
          {connected && <button className="secondary" onClick={() => preview(true)} disabled={!!busy}>Disconnect</button>}
        </div>
      </div>
      {row.installed && row.project_config && !row.project_connected && (
        // AUD-P2-29: the status is for this folder; another project may well be connected
        <p className="muted small">
          {row.user_connected
            ? `Connected for all projects. Connecting here as well adds an entry for this folder only.`
            : `Connections are per project folder: ${row.name} may be connected in another project, but not in ${folderName(project) || "this one"}. Connect it here, or choose "All projects" to connect it everywhere.`}
        </p>
      )}
      {row.installed && (() => {
        const l = loopLine(row, connected);
        return <p className={`small loop ${l.ok ? "on" : "off"}`}>{l.text}</p>;
      })()}
      {needsProject && <p className="muted small">Choose a project first, or connect for all projects.</p>}
      {busy && <p className="muted small">{busy}…</p>}
      {error && <div className="problem" role="alert">{error}</div>}
      {done && <p className="ok small">{done}</p>}
      {plan && (
        <div className="agent-plan">
          {plan.changes.length === 0 && plan.client_exists ? (
            <p className="small">Nothing to change: {row.name} is already {plan.remove ? "disconnected" : "connected"} here.</p>
          ) : (
            <>
              <p className="small">
                {plan.remove ? "Disconnecting" : "Connecting"} {row.name} ({plan.scope === "user" ? "all projects" : "this project"}) changes:
              </p>
              {!plan.remove && !plan.client_exists && <p className="small">• a new agent client <code>{row.id}</code> with its own token (no admin rights; new folders and device switches ask you first)</p>}
              {plan.changes.map((c) => (
                // AUD-P2-28: every file's change is shown, not only its name
                <details key={c.path} open>
                  <summary className="small">
                    {c.action} <code>{c.path}</code>
                  </summary>
                  <pre className="diff" tabIndex={0}>{c.diff || "(no text change)"}</pre>
                  {sharedWithPath(c, project) && (
                    <p className="small warn agent-commit-warn">
                      This file is inside your project and holds this PC's path to <code>umr.exe</code>. If the project is shared
                      through git, add <code>{relative(c.path, project)}</code> to <code>.gitignore</code>: everyone connects their
                      own agent, with their own path.
                    </p>
                  )}
                </details>
              ))}
              <div className="row">
                <span />
                <span>
                  <button className="secondary" onClick={() => setPlan(null)}>Cancel</button>{" "}
                  <button onClick={apply} disabled={!!busy}>{plan.remove ? "Disconnect" : "Apply"}</button>
                </span>
              </div>
            </>
          )}
        </div>
      )}
      {report && (
        <div className="agent-plan">
          {report.length === 0 && <p className="small">Not connected.</p>}
          {report.length > 0 && (() => {
            const all = report.flatMap((r) => r.checks);
            const fails = all.filter((k) => k.result === "fail").length;
            const warns = all.filter((k) => k.result === "warn").length;
            return (
              <p className={`small loop ${fails ? "off" : "on"}`}>
                {fails
                  ? `Not working: ${fails} problem${fails === 1 ? "" : "s"} below.`
                  : `Working: ${row.name} reaches the runtime${LOOP[row.id] ? " and its stop hook answers" : ""}.${warns ? ` ${warns} warning${warns === 1 ? "" : "s"} below.` : ""}`}
              </p>
            );
          })()}
          {report.map((r) => (
            <ul key={r.client} className="readiness">
              {r.checks.map((k, i) => (
                <li key={i}>
                  <span className={k.result === "ok" ? "ok" : "warn"}>{k.result === "ok" ? "OK" : k.result === "warn" ? "Warning" : "Problem"}</span>
                  {k.what}
                  {k.result !== "ok" && k.fix && <span className="muted small"> — fix: <code>{k.fix}</code></span>}
                </li>
              ))}
              {r.fixed.map((s) => (
                <li key={s}><span className="ok">Fixed</span>{s} scope now points at this umr.exe</li>
              ))}
            </ul>
          ))}
          {report.some((r) => r.checks.some((k) => k.fix?.includes("--fix"))) && (
            <button onClick={() => doctor(true)} disabled={!!busy}>Repair</button>
          )}
        </div>
      )}
    </div>
  );
}
