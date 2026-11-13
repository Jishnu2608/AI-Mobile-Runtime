// DIA-004: Problems and Logs panels of the preview window. Problems come classified and
// deduplicated from the Core (`diagnostics.list`), logs from `logs.tail`; both already sanitised.
import { useEffect, useRef, useState } from "react";
import { call } from "./api";

export interface Problem {
  fingerprint: string;
  category: string;
  origin: string;
  severity: string;
  source: string;
  message: string;
  file?: string;
  line?: number;
  count: number;
  seq: number;
  first_seen_ms: number;
  stack_frames: number;
  stack?: string[];
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

/** Problems of the current run, kept current by polling with a cursor. */
export function useProblems(): Problem[] {
  const [items, setItems] = useState<Map<string, Problem>>(new Map());
  useEffect(() => {
    let cursor = 0;
    let known = new Map<string, Problem>();
    let stop = false;
    const tick = async () => {
      try {
        const r = await call<{ entries: Problem[]; cursor: number; total: number }>("diagnostics.list", { since: cursor, limit: 500 });
        if (stop) return;
        // a new run starts empty: fetch everything again
        if (r.total < known.size) {
          known = new Map();
          cursor = 0;
          return tick();
        }
        if (r.entries.length) {
          known = new Map(known);
          for (const e of r.entries) known.set(e.fingerprint, e);
          setItems(known);
        }
        cursor = r.cursor;
      } catch {
        /* Core restarting: try again */
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
  return [...items.values()].sort((a, b) => a.first_seen_ms - b.first_seen_ms);
}

function where(p: Problem) {
  if (!p.file) return "";
  return p.line ? `${p.file}:${p.line}` : p.file;
}

function asText(p: Problem, stack: string[]) {
  return [`${p.category} (${p.origin}) ${p.source}: ${p.message}`, where(p) && `at ${where(p)}`, ...stack.map((f) => `    ${f}`)]
    .filter(Boolean)
    .join("\n");
}

function ProblemRow({ p }: { p: Problem }) {
  const [open, setOpen] = useState(false);
  const [stack, setStack] = useState<string[] | null>(null);
  const [copied, setCopied] = useState(false);
  const toggle = async () => {
    setOpen(!open);
    if (!open && stack === null && p.stack_frames > 0) {
      try {
        const full = await call<Problem>("diagnostics.get", { fingerprint: p.fingerprint });
        setStack(full.stack ?? []);
      } catch {
        setStack([]);
      }
    }
  };
  const copy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(asText(p, stack ?? []));
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard refused */
    }
  };
  return (
    <div className={`dg-row ${p.severity}`} onClick={toggle}>
      <div className="dg-head">
        <span className={`dg-origin ${p.origin}`} title={p.origin === "app" ? "in your code" : p.origin === "environment" ? "toolchain, device or network" : "origin unknown"}>
          {p.origin === "app" ? "code" : p.origin === "environment" ? "env" : "?"}
        </span>
        <span className="dg-msg">{p.message.split("\n")[0]}</span>
        {p.count > 1 && <span className="dg-count">×{p.count}</span>}
        <button className="dg-copy" title="Copy (for your agent)" onClick={copy}>{copied ? "✓" : "⧉"}</button>
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
          {stack === null && p.stack_frames > 0 ? "loading…" : (stack ?? []).join("\n")}
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
            <div className="dg-cat">{cat} · {group.length}</div>
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
