// AUD-P3-24: the home screen for returning users. The setup wizard was a one-time installer;
// this is what the app opens on once a project has run: what runs now, recent projects with
// one-click Run, the current problems, and what the coding agent was told and fixed.
import { useEffect, useState } from "react";
import { Bot, CircleAlert, FolderOpen, Play, Plus, Smartphone, Wrench } from "lucide-react";
import { call, openPreview } from "./api";

interface Recent {
  path: string;
  adapter: string;
  last_run_unix: number;
  runs: number;
  exists: boolean;
}
interface FeedItem {
  at_ms: number;
  kind: "told" | "fixed";
  message: string;
  seconds?: number;
}
export interface HomeSummary {
  recent: Recent[];
  session: { state: string; profile: string; app?: string | null; serial?: string | null; project?: string | null } | null;
  run: { project: string; adapter: string; package: string; serial: string } | null;
  problems: { total: number; by_category: Record<string, number> };
  feed: FeedItem[];
  fixed_today: number;
}

const FRAMEWORK: Record<string, string> = { "gradle-android": "Kotlin / Java", flutter: "Flutter", "react-native": "React Native" };
const STATE: Record<string, string> = {
  running: "Running",
  reloading: "Reloading",
  building: "Building",
  launching: "Starting",
  booting: "Starting the emulator",
  build_failed: "Build failed",
  crashed: "Crashed",
  launch_failed: "Did not start",
  device_error: "Emulator problem",
  stopped: "Stopped",
  idle: "Idle",
};

function ago(unixSeconds: number): string {
  const s = Math.max(0, Date.now() / 1000 - unixSeconds);
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

function name(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
}

function time(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function useHome(): HomeSummary | null {
  const [h, setH] = useState<HomeSummary | null>(null);
  useEffect(() => {
    let stop = false;
    const tick = () =>
      call<HomeSummary>("home.summary")
        .then((v) => !stop && setH(v))
        .catch(() => undefined);
    tick();
    const t = window.setInterval(tick, 3000);
    return () => {
      stop = true;
      window.clearInterval(t);
    };
  }, []);
  return h;
}

export function Home({ summary, onSetup, onNewProject, onAgents }: { summary: HomeSummary | null; onSetup: (path: string) => void; onNewProject: () => void; onAgents: () => void }) {
  const [running, setRunning] = useState<string | null>(null);
  const [result, setResult] = useState<Record<string, string>>({});
  if (!summary) return <p className="busy">Loading…</p>;
  const { session, run, problems, feed, recent } = summary;
  const live = !!run && !!session && !["stopped", "idle"].includes(session.state);

  const runProject = async (path: string) => {
    setRunning(path);
    setResult((r) => ({ ...r, [path]: "" }));
    try {
      const v = await call<{ status: string; error?: string; seconds?: number }>("project.run", { path, watch: true });
      if (v.status === "running") {
        openPreview().catch(() => undefined);
        setResult((r) => ({ ...r, [path]: `Running (${v.seconds ?? "?"} s)` }));
      } else setResult((r) => ({ ...r, [path]: `Did not start: ${v.error ?? v.status}` }));
    } catch (e) {
      setResult((r) => ({ ...r, [path]: String(e) }));
    } finally {
      setRunning(null);
    }
  };

  return (
    <section className="home">
      <div className="card home-now">
        <div className="home-row">
          <Smartphone aria-hidden />
          <div className="home-grow">
            <p className="big">
              {live ? (
                <>
                  <span className={session!.state === "running" ? "ok" : "warn"}>{STATE[session!.state] ?? session!.state}</span> {name(run!.project)}
                </>
              ) : (
                "Nothing is running"
              )}
            </p>
            <p className="small muted">
              {live ? `${FRAMEWORK[run!.adapter] ?? run!.adapter} · ${run!.package} on ${session!.serial ?? run!.serial} (${session!.profile} size)` : "Run a recent project below, or set up a new one."}
            </p>
          </div>
          {live && (
            <button className="primary" onClick={() => openPreview().catch(() => undefined)}>
              Open the live preview
            </button>
          )}
        </div>
        {live && problems.total > 0 && (
          <p className="home-problems">
            <CircleAlert aria-hidden /> {problems.total} problem{problems.total === 1 ? "" : "s"} now:{" "}
            {Object.entries(problems.by_category)
              .map(([c, n]) => `${n} ${c}`)
              .join(", ")}{" "}
            — open the live preview to see them.
          </p>
        )}
      </div>

      <h3>Recent projects</h3>
      {recent.length === 0 && <p className="muted">No project has run yet.</p>}
      <ul className="home-list">
        {recent.map((r) => (
          <li key={r.path} className="card home-row">
            <FolderOpen aria-hidden />
            <div className="home-grow">
              <p className="big">{name(r.path)}</p>
              <p className="small muted" title={r.path}>
                {FRAMEWORK[r.adapter] ?? r.adapter} · last run {ago(r.last_run_unix)} · {r.runs} run{r.runs === 1 ? "" : "s"}
                {!r.exists && " · folder not found"}
              </p>
              {result[r.path] && <p className="small">{result[r.path]}</p>}
            </div>
            <button className="secondary" onClick={() => onSetup(r.path)} title="Check what this project needs (Plan)">
              <Wrench aria-hidden /> Check
            </button>
            <button className="primary" disabled={!r.exists || !!running} onClick={() => runProject(r.path)}>
              <Play aria-hidden /> {running === r.path ? "Starting…" : "Run"}
            </button>
          </li>
        ))}
      </ul>
      <p>
        <button className="secondary" onClick={onNewProject}>
          <Plus aria-hidden /> Set up a new project
        </button>
      </p>

      <h3>Your agent's work</h3>
      {summary.fixed_today > 0 && (
        <p className="ok">
          Your agent fixed {summary.fixed_today} problem{summary.fixed_today === 1 ? "" : "s"} in the last 24 hours.
        </p>
      )}
      {feed.length === 0 ? (
        <p className="muted">
          Nothing yet. When a connected coding agent finishes a task and the app has a problem, it is told here and keeps fixing.{" "}
          <button className="link" onClick={onAgents}>
            Connect your agent
          </button>
        </p>
      ) : (
        <ul className="home-feed">
          {feed.map((f, i) => (
            <li key={i} className={f.kind}>
              <Bot aria-hidden />
              <span className="muted small">{time(f.at_ms)}</span>
              <span>
                {f.kind === "told" ? "Told your agent: " : `Fixed${f.seconds !== undefined ? ` after ${f.seconds < 120 ? `${f.seconds} s` : `${Math.round(f.seconds / 60)} min`}` : ""}: `}
                {f.message}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
