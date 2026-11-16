// DIA-004: Problems and Logs panels of the preview window. Problems come classified and
// deduplicated from the Core (`diagnostics.list`), logs from `logs.tail`; both already sanitised.
import { useEffect, useRef, useState } from "react";
import { Check, ClipboardCopy, Copy } from "lucide-react";
import { call, openDocs, troubleshootingAnchor } from "./api";

export interface Problem {
  fingerprint: string;
  category: string;
  origin: string;
  severity: string;
  source: string;
  message: string;
  file?: string;
  line?: number;
  column?: number;
  count: number;
  seq: number;
  first_seen_ms: number;
  stack_frames: number;
  stack?: string[];
  /** The source lines around `file:line` (`diagnostics.get` only). */
  code?: { first: number; line: number; lines: string[] };
}

interface LogLine {
  seq: number;
  source: string;
  level: string;
  tag?: string;
  pid?: number;
  text: string;
}

const ORDER = ["crash", "build", "runtime", "environment", "device", "network"];
const KEEP_LOGS = 1500;

/** Identity of a list: which problems, how often, and when last seen. */
export function listKey(entries: Problem[]): string {
  return entries.map((e) => `${e.fingerprint}:${e.count}:${e.seq}`).join("|");
}

/**
 * Problems of the current run, the same list `umr problems` shows (AUD-P1-3). Each poll reads the
 * whole list instead of only what is new: problems that were fixed (dropped when a reload starts)
 * leave the panel, and a run of another project, which starts with an empty list, replaces the
 * old one. The list is small (at most 500, deduplicated) and comes from this PC.
 */
export function useProblems(): Problem[] {
  const [items, setItems] = useState<Problem[]>([]);
  useEffect(() => {
    let shown = "";
    let stop = false;
    const tick = async () => {
      try {
        const r = await call<{ entries: Problem[] }>("diagnostics.list", { limit: 500 });
        if (stop) return;
        const entries = r.entries ?? [];
        const key = listKey(entries);
        if (key !== shown) {
          shown = key;
          setItems(entries);
        }
      } catch {
        /* Core restarting: keep what is shown and try again */
      }
    };
    const loop = async () => {
      while (!stop) {
        await tick();
        await new Promise((r) => setTimeout(r, 1500));
      }
    };
    loop();
    return () => {
      stop = true;
    };
  }, []);
  return [...items].sort((a, b) => a.first_seen_ms - b.first_seen_ms);
}

function where(p: Problem) {
  if (!p.file) return "";
  return p.line ? `${p.file}:${p.line}` : p.file;
}

const ORIGIN_TEXT: Record<string, string> = { app: "in your code", environment: "toolchain, device or network", unknown: "origin unknown" };

/** React Native frames name the dev server's bundle URL with a long query: keep the bundle name. */
export function shortFrame(f: string): string {
  return f.replace(/https?:\/\/[^\s)]*?\/([\w.-]+\.bundle)[^\s)]*?(:\d+:\d+)(?=\))/g, "$1$2");
}

/** The code around the problem, numbered, with the problem's line marked `>`. */
export function codeText(code: Problem["code"]): string[] {
  if (!code?.lines.length) return [];
  const width = String(code.first + code.lines.length - 1).length;
  return code.lines.map((l, i) => {
    const n = code.first + i;
    return `${n === code.line ? ">" : " "} ${String(n).padStart(width)} | ${l}`;
  });
}

/**
 * AUD-P1-11: one problem as plain text for an agent, a forum or a colleague: what, where, how
 * often, the stack and the code around it. `p` is the full entry from `diagnostics.get`.
 */
export function problemText(p: Problem): string {
  const loc = where(p) && (p.column ? `${where(p)}:${p.column}` : where(p));
  const out = [
    `${p.category.toUpperCase()} · ${p.severity} · ${ORIGIN_TEXT[p.origin] ?? p.origin} · ${p.source}${p.count > 1 ? ` · seen ${p.count}×` : ""}`,
    p.message,
  ];
  if (loc) out.push(`at ${loc}`);
  const code = codeText(p.code);
  if (code.length) out.push("", "Code:", ...code);
  const stack = (p.stack ?? []).map(shortFrame);
  if (stack.length) {
    out.push("", "Stack:", ...stack.slice(0, 25).map((f) => `  ${f}`));
    if (stack.length > 25) out.push(`  … ${stack.length - 25} more frames`);
  }
  return out.join("\n");
}

interface RunContext {
  project?: string;
  adapter?: string;
  package?: string;
  serial?: string;
  avd?: string;
  profile?: string;
}

/** All problems, with what someone outside needs to know about the app and the device. */
export function reportText(problems: Problem[], run: RunContext, when = new Date()): string {
  const head = [`AI Mobile Runtime — ${problems.length} problem${problems.length === 1 ? "" : "s"} in the current run`];
  if (run.project) head.push(`Project: ${run.project}${run.adapter ? ` (${run.adapter})` : ""}`);
  if (run.package) {
    const device = [run.serial, run.avd && `emulator ${run.avd}`, run.profile && `${run.profile} size`].filter(Boolean).join(", ");
    head.push(`App: ${run.package}${device ? ` on ${device}` : ""}`);
  }
  head.push(`Copied: ${when.toISOString().replace("T", " ").slice(0, 16)} UTC`);
  const body = problems.map((p, i) => `${i + 1}. ${problemText(p).replace(/\n/g, "\n   ")}`);
  return [head.join("\n"), ...body].join("\n\n") + "\n";
}

/** The full entry (stack and code) of a listed problem; the listed one if that fails. */
async function full(p: Problem): Promise<Problem> {
  try {
    return { ...p, ...(await call<Problem>("diagnostics.get", { fingerprint: p.fingerprint })) };
  } catch {
    return p;
  }
}

/** Copies `text`; true when the clipboard took it. */
async function toClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Copy all problems with project, app and device (AUD-P1-11). */
function CopyAll({ problems }: { problems: Problem[] }) {
  const [state, setState] = useState<"idle" | "busy" | "done" | "failed">("idle");
  const copy = async () => {
    setState("busy");
    const run: RunContext = {};
    try {
      const l = await call<{ run?: RunContext }>("diagnostics.list", { limit: 1 });
      Object.assign(run, l.run ?? {});
    } catch {
      /* without the run: the problems alone */
    }
    try {
      const s = await call<{ session?: { avd?: string; profile?: string } | null }>("session.status");
      run.avd = s.session?.avd;
      run.profile = s.session?.profile;
    } catch {
      /* without the device name */
    }
    const all: Problem[] = [];
    for (const p of problems.slice(0, 50)) all.push(await full(p));
    const ok = await toClipboard(reportText(all, run));
    setState(ok ? "done" : "failed");
    setTimeout(() => setState("idle"), 1600);
  };
  return (
    <button className="dg-copyall" onClick={copy} disabled={state === "busy"} title="Copy every problem with the project, app, device, code and stack — to paste into an agent chat, an issue or a forum">
      {state === "done" ? <Check aria-hidden /> : <ClipboardCopy aria-hidden />}
      {state === "done" ? "Copied" : state === "failed" ? "Copy failed" : state === "busy" ? "Copying…" : "Copy all"}
    </button>
  );
}

function ProblemRow({ p }: { p: Problem }) {
  const [open, setOpen] = useState(false);
  // the full entry (stack and code), fetched when the row opens or is copied
  const [detail, setDetail] = useState<Problem | null>(null);
  const [copied, setCopied] = useState<"" | "done" | "failed">("");
  const load = async () => {
    const d = await full(p);
    setDetail(d);
    return d;
  };
  const toggle = () => {
    setOpen(!open);
    if (!open && !detail) load();
  };
  const copy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    const ok = await toClipboard(problemText(detail ?? (await load())));
    setCopied(ok ? "done" : "failed");
    setTimeout(() => setCopied(""), 1400);
  };
  const code = codeText(detail?.code);
  const stack = (detail?.stack ?? []).map(shortFrame);
  return (
    <div className={`dg-row ${p.severity}`} onClick={toggle}>
      <div className="dg-head">
        <span className={`dg-origin ${p.origin}`} title={ORIGIN_TEXT[p.origin] ?? p.origin}>
          {p.origin === "app" ? "code" : p.origin === "environment" ? "env" : "?"}
        </span>
        <span className="dg-msg">{p.message.split("\n")[0]}</span>
        {p.count > 1 && <span className="dg-count">×{p.count}</span>}
        <button className={`dg-copy${copied ? " " + copied : ""}`} title="Copy this problem with its place, code and stack — for your agent, an issue or a forum" onClick={copy}>
          {copied === "done" ? <Check aria-hidden /> : <Copy aria-hidden />}
          {copied === "done" ? "Copied" : copied === "failed" ? "Failed" : "Copy"}
        </button>
      </div>
      {(where(p) || open) && (
        <div className="dg-where">
          {where(p)}
          {where(p) && " · "}
          {p.source}
        </div>
      )}
      {open && (
        <pre className="dg-stack">
          {p.message.includes("\n") && p.message + "\n"}
          {!detail ? "loading…" : [...(code.length ? [...code, ""] : []), ...stack].join("\n")}
        </pre>
      )}
    </div>
  );
}

function ProblemsTab({ problems }: { problems: Problem[] }) {
  if (!problems.length) return <div className="dg-empty">No problems in the current run.</div>;
  return (
    <div className="dg-list">
      {ORDER.map((cat) => {
        const group = problems.filter((p) => p.category === cat);
        if (!group.length) return null;
        return (
          <div key={cat}>
            <div className="dg-cat">
              {cat} · {group.length}
              {troubleshootingAnchor(cat) && (
                <button className="dg-help" title="Open the troubleshooting guide for this category" onClick={() => openDocs("troubleshooting", troubleshootingAnchor(cat)).catch(() => undefined)}>
                  How to fix ↗
                </button>
              )}
            </div>
            {group.map((p) => <ProblemRow key={p.fingerprint} p={p} />)}
          </div>
        );
      })}
    </div>
  );
}

function LogsTab() {
  const [lines, setLines] = useState<LogLine[]>([]);
  const [filter, setFilter] = useState("");
  const [source, setSource] = useState("");
  const [level, setLevel] = useState("");
  const [paused, setPaused] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;
  useEffect(() => {
    let cursor = 0;
    let stop = false;
    const loop = async () => {
      while (!stop) {
        if (!pausedRef.current) {
          try {
            const r = await call<{ lines: LogLine[]; cursor: number }>("logs.tail", { since: cursor, limit: KEEP_LOGS });
            if (stop) return;
            if (r.cursor < cursor) cursor = 0;
            else if (r.lines.length) {
              setLines((old) => [...old, ...r.lines].slice(-KEEP_LOGS));
              cursor = r.cursor;
            }
          } catch {
            /* Core restarting */
          }
        }
        await new Promise((r) => setTimeout(r, 1000));
      }
    };
    loop();
    return () => {
      stop = true;
    };
  }, []);
  // follow the end unless the user scrolled up
  useEffect(() => {
    const b = box.current;
    if (b && b.scrollHeight - b.scrollTop - b.clientHeight < 60) b.scrollTop = b.scrollHeight;
  }, [lines]);
  const rank = (l: string) => "VDIWEF".indexOf(l.toUpperCase());
  const f = filter.toLowerCase();
  const shown = lines.filter(
    (l) =>
      (!source || l.source === source) &&
      (!level || rank(l.level) >= rank(level)) &&
      (!f || l.text.toLowerCase().includes(f) || (l.tag ?? "").toLowerCase().includes(f)),
  );
  return (
    <div className="dg-logs">
      <div className="dg-filters">
        <input placeholder="filter" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <select value={source} onChange={(e) => setSource(e.target.value)} title="Source">
          <option value="">all</option>
          <option value="logcat">app</option>
          <option value="tool">tool</option>
          <option value="umr">umr</option>
        </select>
        <select value={level} onChange={(e) => setLevel(e.target.value)} title="Lowest level">
          <option value="">any</option>
          <option value="I">I+</option>
          <option value="W">W+</option>
          <option value="E">E+</option>
        </select>
        <button onClick={() => setPaused(!paused)} title={paused ? "Resume" : "Pause"}>{paused ? "▶" : "❚❚"}</button>
        <button onClick={() => setLines([])} title="Clear the view">✕</button>
      </div>
      <div className="dg-lines" ref={box}>
        {shown.map((l) => (
          <div key={l.seq} className={`dg-line lv-${l.level}`}>
            <span className="dg-lv">{l.level}</span>
            {l.tag && <span className="dg-tag">{l.tag}</span>}
            {l.text}
          </div>
        ))}
        {!shown.length && <div className="dg-empty">No log lines yet.</div>}
      </div>
    </div>
  );
}

export function DiagnosticsPanel({ problems, onClose }: { problems: Problem[]; onClose: () => void }) {
  const [tab, setTab] = useState<"problems" | "logs">("problems");
  return (
    <div className="dg">
      <div className="dg-tabs">
        <button className={tab === "problems" ? "on" : ""} onClick={() => setTab("problems")}>Problems {problems.length ? `(${problems.length})` : ""}</button>
        <button className={tab === "logs" ? "on" : ""} onClick={() => setTab("logs")}>Logs</button>
        {tab === "problems" && problems.length > 0 && <CopyAll problems={problems} />}
        <button className="dg-close" title="Close" onClick={onClose}>✕</button>
      </div>
      {tab === "problems" ? <ProblemsTab problems={problems} /> : <LogsTab />}
    </div>
  );
}

interface Approval {
  id: string;
  client: string;
  action: string;
  detail: string;
}

/** MCP-003: agent requests waiting for the user (polls `approvals.list`). */
export function Approvals() {
  const [pending, setPending] = useState<Approval[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let stop = false;
    const loop = async () => {
      while (!stop) {
        try {
          const r = await call<{ pending: Approval[] }>("approvals.list");
          if (stop) return;
          setPending(r.pending ?? []);
        } catch {
          /* Core restarting */
        }
        await new Promise((r) => setTimeout(r, 1500));
      }
    };
    loop();
    return () => {
      stop = true;
    };
  }, []);
  const decide = async (id: string, decision: "allow" | "deny", always = false) => {
    setBusy(true);
    try {
      await call("approvals.decide", { id, decision, always });
      setPending((p) => p.filter((a) => a.id !== id));
    } catch {
      /* already answered or timed out */
    } finally {
      setBusy(false);
    }
  };
  if (!pending.length) return null;
  const a = pending[0];
  return (
    <div className="ap" role="alertdialog" aria-label="Agent request">
      <div className="ap-text">
        <b>{a.client}</b> wants to {a.action}: <span className="ap-detail">{a.detail}</span>
        {pending.length > 1 && <span className="ap-more"> (+{pending.length - 1} more)</span>}
      </div>
      <div className="ap-actions">
        <button disabled={busy} onClick={() => decide(a.id, "allow")}>Allow</button>
        <button disabled={busy} onClick={() => decide(a.id, "allow", true)} title="Do not ask again for this agent">Always</button>
        <button disabled={busy} className="ap-deny" onClick={() => decide(a.id, "deny")}>Deny</button>
      </div>
    </div>
  );
}
