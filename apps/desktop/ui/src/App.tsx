import { useEffect, useRef, useState } from "react";
import { call, inTauri, loadWizard, saveWizard, size, Step, WizardState } from "./api";

const STEPS: { id: Step; label: string }[] = [
  { id: "scan", label: "Scan" },
  { id: "project", label: "Project" },
  { id: "plan", label: "Plan" },
  { id: "licences", label: "Licences" },
  { id: "install", label: "Install" },
  { id: "ready", label: "Ready" },
];

const FRAMEWORK: Record<string, string> = {
  "android-native": "Android (Kotlin/Java)",
  flutter: "Flutter",
  "react-native": "React Native",
  expo: "Expo",
  unknown: "Not recognised",
  "expo-go": "Expo Go",
};

function useAsync<T>(fn: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const run = () => {
    setLoading(true);
    setError(null);
    fn()
      .then(setData)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  };
  useEffect(run, deps); // eslint-disable-line react-hooks/exhaustive-deps
  return { data, error, loading, reload: run };
}

function Problem({ text, retry }: { text: string; retry?: () => void }) {
  return (
    <div className="problem" role="alert">
      <strong>Something went wrong.</strong> {text}
      {retry && (
        <button className="secondary" onClick={retry}>
          Try again
        </button>
      )}
    </div>
  );
}

function Busy({ text }: { text: string }) {
  return <p className="busy">{text}</p>;
}

export function App() {
  const [state, setState] = useState<WizardState | null>(null);

  useEffect(() => {
    loadWizard()
      .then((s) => setState(s ?? { step: "scan", projectPath: "" }))
      .catch(() => setState({ step: "scan", projectPath: "" }));
  }, []);

  const go = (next: Partial<WizardState>) => {
    setState((s) => {
      const merged = { ...(s as WizardState), ...next };
      saveWizard(merged).catch(() => undefined);
      return merged;
    });
  };

  if (!state) return <Busy text="Starting…" />;
  const index = STEPS.findIndex((s) => s.id === state.step);

  return (
    <div className="shell">
      <header>
        <h1>AI Mobile Runtime</h1>
        <p className="sub">Set up everything your project needs to run on the Android emulator.</p>
        {!inTauri && <p className="demo">Browser preview with sample data — open the desktop app for real results.</p>}
      </header>
      <ol className="steps" aria-label="Setup steps">
        {STEPS.map((s, i) => (
          <li key={s.id} className={i < index ? "done" : i === index ? "current" : ""} aria-current={i === index ? "step" : undefined}>
            <span className="dot">{i < index ? "✓" : i + 1}</span>
            {s.label}
          </li>
        ))}
      </ol>
      <main>
        {state.step === "scan" && <ScanStep onNext={() => go({ step: "project" })} />}
        {state.step === "project" && <ProjectStep path={state.projectPath} onBack={() => go({ step: "scan" })} onNext={(p) => go({ step: "plan", projectPath: p })} />}
        {state.step === "plan" && (
          <PlanStep path={state.projectPath} onBack={() => go({ step: "project" })} onNext={(needsLicences, nothing) => go({ step: nothing ? "ready" : needsLicences ? "licences" : "install" })} />
        )}
        {state.step === "licences" && <LicenceStep path={state.projectPath} onBack={() => go({ step: "plan" })} onNext={() => go({ step: "install" })} />}
        {state.step === "install" && <InstallStep path={state.projectPath} jobId={state.jobId} onJob={(jobId) => go({ jobId })} onBack={() => go({ step: "plan", jobId: undefined })} onDone={() => go({ step: "ready", jobId: undefined })} />}
        {state.step === "ready" && <ReadyStep path={state.projectPath} onRestart={() => go({ step: "scan", jobId: undefined })} />}
      </main>
    </div>
  );
}

function ScanStep({ onNext }: { onNext: () => void }) {
  const { data, error, loading, reload } = useAsync(() => call("env.scan"), []);
  if (loading) return <Busy text="Scanning this computer (a few seconds)…" />;
  if (error) return <Problem text={error} retry={reload} />;
  const r: any = data;
  const v = r.host.virtualization;
  return (
    <section>
      <h2>This computer</h2>
      <div className="facts">
        <Fact label="Windows" value={`build ${r.host.windows_build}${r.host.windows_11 ? " (Windows 11)" : ""}`} ok={r.host.windows_11} />
        <Fact label="Memory" value={size(r.host.ram_total_bytes)} ok={r.host.ram_total_bytes >= 8 * 1073741824} />
        <Fact label="Free disk (toolchains)" value={size(r.host.disk_free_bytes)} ok={r.host.disk_free_bytes >= 20 * 1073741824} />
        <Fact label="Emulator acceleration" value={v.acceleration_available ? "available" : `not available (${v.whpx})`} ok={v.acceleration_available} />
        <Fact label="JDKs found" value={String(r.jdks.length)} ok />
        <Fact label="Android SDKs found" value={String(r.android_sdks.length)} ok />
      </div>
      <h3>Ready to build</h3>
      <ul className="readiness">
        {r.readiness.map((p: any) => (
          <li key={p.profile}>
            <span className={p.ready ? "ok" : "warn"}>{p.ready ? "Ready" : "Needs setup"}</span> {FRAMEWORK[p.profile.replace("-android", "")] ?? p.profile}
            {!p.ready && <span className="muted"> — missing: {p.missing.join(", ")}</span>}
          </li>
        ))}
      </ul>
      <Nav onNext={onNext} nextLabel="Choose project" />
    </section>
  );
}

function Fact({ label, value, ok }: { label: string; value: string; ok: boolean }) {
  return (
    <div className="fact">
      <span className="label">{label}</span>
      <span className={ok ? "value" : "value warn"}>{value}</span>
    </div>
  );
}

function ProjectStep({ path, onBack, onNext }: { path: string; onBack: () => void; onNext: (p: string) => void }) {
  const [input, setInput] = useState(path);
  const [result, setResult] = useState<any>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const detect = (p: string) => {
    setBusy(true);
    setError(null);
    call("project.detect", { path: p.trim() })
      .then(setResult)
      .catch((e) => {
        setResult(null);
        setError(String(e));
      })
      .finally(() => setBusy(false));
  };
  useEffect(() => {
    if (path) detect(path);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const trust = () => {
    if (!window.confirm("Building runs the project's own code (Gradle scripts, npm install scripts, Flutter plugins). Trust this folder?")) return;
    call("project.trust", { path: input.trim(), trusted: true }).then(() => detect(input));
  };
  const d = result?.detection;
  return (
    <section>
      <h2>Your project</h2>
      <label htmlFor="path">Project folder (full path)</label>
      <div className="row">
        <input id="path" value={input} placeholder="D:\Work\my-app" onChange={(e) => setInput(e.target.value)} onKeyDown={(e) => e.key === "Enter" && detect(input)} />
        <button className="secondary" onClick={() => detect(input)} disabled={!input.trim() || busy}>
          {busy ? "Checking…" : "Check"}
        </button>
      </div>
      {error && <Problem text={error} />}
      {d && (
        <div className="card">
          <p className="big">
            {FRAMEWORK[d.framework] ?? d.framework} <span className="muted">· {d.confidence} % sure</span>
          </p>
          <ul className="evidence">
            {d.evidence.map((e: any, i: number) => (
              <li key={i}>
                <code>{e.file}</code> — {e.says}
              </li>
            ))}
          </ul>
          {d.candidates.length > 0 && (
            <>
              <p>Projects inside this folder:</p>
              <ul>
                {d.candidates.map((c: any) => (
                  <li key={c.path}>
                    <button className="link" onClick={() => { const p = `${input.replace(/[\\/]+$/, "")}\\${c.path}`; setInput(p); detect(p); }}>
                      {c.path}
                    </button>{" "}
                    {FRAMEWORK[c.framework]} ({c.confidence} %)
                  </li>
                ))}
              </ul>
            </>
          )}
          <p className="muted">
            {result.trusted ? "Trusted: builds may run this project's scripts." : "Not trusted yet — you will be asked before the first build."}
            {!result.trusted && d.framework !== "unknown" && (
              <button className="link" onClick={trust}>
                Trust now
              </button>
            )}
          </p>
        </div>
      )}
      <Nav onBack={onBack} onNext={() => onNext(input.trim())} nextDisabled={!d || d.framework === "unknown"} nextLabel="See what is needed" />
    </section>
  );
}

function PlanStep({ path, onBack, onNext }: { path: string; onBack: () => void; onNext: (needsLicences: boolean, nothing: boolean) => void }) {
  const { data, error, loading, reload } = useAsync(() => call("install.prepare", { path }), [path]);
  const [admin, setAdmin] = useState<Record<string, string>>({});
  if (loading) return <Busy text="Reading the project and official release information…" />;
  if (error) return <Problem text={error} retry={reload} />;
  const plan = (data as any).plan;
  const prepared = (data as any).prepared;
  const installs = plan.items.filter((i: any) => i.action.kind === "install");
  const runAdmin = (id: string) =>
    call("admin.run", { action: id })
      .then((r: any) => setAdmin((a) => ({ ...a, [id]: `${r.status}: ${r.message}` })))
      .catch((e) => setAdmin((a) => ({ ...a, [id]: String(e) })));
  const blocking = plan.admin_actions.some((a: any) => a.required && !admin[a.id]?.startsWith("done") && !admin[a.id]?.startsWith("already"));
  return (
    <section>
      <h2>What this project needs</h2>
      <table className="plan">
        <tbody>
          {plan.items.map((i: any) => (
            <tr key={i.component + (i.package ?? "")}>
              <td>
                <span className={i.action.kind === "install" ? "tag install" : "tag reuse"}>{i.action.kind === "install" ? "Install" : "Reuse"}</span>
              </td>
              <td>
                {i.title} {i.version && <span className="muted">{i.version}</span>}
                <div className="muted small">{i.reason}</div>
              </td>
              <td className="num">{i.action.kind === "install" ? `~${size(i.approx_download_bytes)}` : ""}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p>
        Download ~{size(plan.total_download_bytes)} · free {size(plan.disk_free_bytes)}
        {!plan.disk_ok && <strong className="warn"> — not enough free space</strong>}
      </p>
      {plan.admin_actions.length > 0 && (
        <div className="card">
          <h3>Needs administrator permission</h3>
          {plan.admin_actions.map((a: any) => (
            <div key={a.id} className="row">
              <div>
                <strong>{a.title}</strong> {a.required ? "(required)" : "(recommended)"}
                <div className="muted small">
                  {a.reason}
                  {a.reboot ? " · restart needed" : ""}
                </div>
                {admin[a.id] && <div className="small">{admin[a.id]}</div>}
              </div>
              <button className="secondary" onClick={() => runAdmin(a.id)}>
                Turn on…
              </button>
            </div>
          ))}
        </div>
      )}
      {plan.conflicts.length > 0 && (
        <div className="card">
          <h3>Problems in the project (not changed by us)</h3>
          {plan.conflicts.map((c: any) => (
            <p key={c.id}>
              {c.message}
              <br />
              <span className="muted small">Fix: {c.hint}</span>
            </p>
          ))}
        </div>
      )}
      {installs.length === 0 ? (
        <Nav onBack={onBack} onNext={() => onNext(false, true)} nextLabel="Everything is ready" />
      ) : (
        <Nav onBack={onBack} onNext={() => onNext(prepared.licences.some((l: any) => !l.accepted), false)} nextDisabled={!plan.disk_ok || blocking} nextLabel={`Continue (${installs.length} to install)`} />
      )}
    </section>
  );
}

function LicenceStep({ path, onBack, onNext }: { path: string; onBack: () => void; onNext: () => void }) {
  const { data, error, loading, reload } = useAsync(() => call("install.prepare", { path }), [path]);
  const [checked, setChecked] = useState<Record<string, boolean>>({});
  const [done, setDone] = useState<Record<string, boolean>>({});
  const [failure, setFailure] = useState<string | null>(null);
  if (loading) return <Busy text="Loading licence texts…" />;
  if (error) return <Problem text={error} retry={reload} />;
  const licences = (data as any).prepared.licences.filter((l: any) => !l.accepted);
  const accept = (l: any) =>
    call("install.licence.accept", { id: l.id, sha256: l.sha256 })
      .then(() => setDone((d) => ({ ...d, [l.id]: true })))
      .catch((e) => setFailure(String(e)));
  const all = licences.every((l: any) => done[l.id]);
  return (
    <section>
      <h2>Licences</h2>
      <p>Read each licence. Nothing is installed until you accept them.</p>
      {failure && <Problem text={failure} />}
      {licences.map((l: any) => (
        <div key={l.id} className="card">
          <h3>{l.title}</h3>
          <pre className="licence" tabIndex={0} aria-label={`${l.title} full text`}>
            {l.text}
          </pre>
          {l.url && <p className="muted small">Also at {l.url}</p>}
          {done[l.id] ? (
            <p className="ok">Accepted.</p>
          ) : (
            <div className="row">
              <label className="check">
                <input type="checkbox" checked={!!checked[l.id]} onChange={(e) => setChecked((c) => ({ ...c, [l.id]: e.target.checked }))} /> I have read and accept the {l.title}
              </label>
              <button onClick={() => accept(l)} disabled={!checked[l.id]}>
                Accept
              </button>
            </div>
          )}
        </div>
      ))}
      <Nav onBack={onBack} onNext={onNext} nextDisabled={!all} nextLabel="Install" />
    </section>
  );
}

function InstallStep({ path, jobId, onJob, onBack, onDone }: { path: string; jobId?: string; onJob: (id: string) => void; onBack: () => void; onDone: () => void }) {
  const [status, setStatus] = useState<any>(null);
  const [error, setError] = useState<string | null>(null);
  const started = useRef(false);

  useEffect(() => {
    let stop = false;
    const poll = async (id: string) => {
      while (!stop) {
        try {
          const st = await call("install.status", { job_id: id });
          setStatus(st);
          if (st.state !== "running") return;
        } catch (e) {
          // job unknown (Core restarted, e.g. after a reboot): start again; downloads resume
          if (!started.current) {
            started.current = true;
            return start();
          }
          setError(String(e));
          return;
        }
        await new Promise((r) => setTimeout(r, 700));
      }
    };
    const start = async () => {
      try {
        const r = await call("install.start", { path });
        onJob(r.job_id);
        await poll(r.job_id);
      } catch (e) {
        setError(String(e));
      }
    };
    if (jobId) poll(jobId);
    else {
      started.current = true;
      start();
    }
    return () => {
      stop = true;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (error) return <Problem text={error} retry={onBack} />;
  if (!status) return <Busy text="Starting the install…" />;
  const word: Record<string, string> = { pending: "waiting", downloading: "downloading", extracting: "extracting", committing: "finishing", installing: "installing", done: "installed", "already-installed": "already installed", failed: "failed", skipped: "skipped" };
  return (
    <section>
      <h2>Installing</h2>
      <ul className="progress">
        {status.items.map((i: any) => {
          const pct = i.bytes_total ? Math.min(100, Math.round((i.bytes_done / i.bytes_total) * 100)) : i.state === "done" || i.state === "already-installed" ? 100 : 0;
          return (
            <li key={i.component + (i.package ?? "")}>
              <div className="row">
                <span>{i.title}</span>
                <span className={i.state === "failed" ? "warn" : "muted"}>
                  {word[i.state] ?? i.state}
                  {i.state === "downloading" && i.bytes_total ? ` ${pct} %` : ""}
                </span>
              </div>
              <div className="bar" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
                <div style={{ width: `${pct}%` }} className={i.state === "installing" ? "pulse" : ""} />
              </div>
              {i.message && <div className="muted small">{i.message}</div>}
            </li>
          );
        })}
      </ul>
      {status.state === "running" && (
        <button className="secondary" onClick={() => call("install.cancel", { job_id: status.id })}>
          Cancel (downloads are kept)
        </button>
      )}
      {status.state === "failed" && <Problem text={`${status.message ?? "Install failed"}. Log: ${status.log}`} retry={onBack} />}
      {status.state === "cancelled" && <p className="muted">Cancelled. You can continue later; downloads resume.</p>}
      {status.state === "done" && <Nav onNext={onDone} nextLabel="Finish" />}
    </section>
  );
}

function ReadyStep({ path, onRestart }: { path: string; onRestart: () => void }) {
  const { data, error, loading, reload } = useAsync(() => call("components.list"), []);
  if (loading) return <Busy text="Checking the installed tools…" />;
  if (error) return <Problem text={error} retry={reload} />;
  const r: any = data;
  const broken = r.components.filter((c: any) => !c.ok);
  return (
    <section>
      <h2>{broken.length ? "Almost ready" : "Ready"}</h2>
      <p>
        {r.components.length} components installed and checked ({size(r.disk.total_bytes)}).
        {broken.length > 0 && <strong className="warn"> {broken.length} need repair — run setup again.</strong>}
      </p>
      <p>
        Project: <code>{path}</code>
      </p>
      <p className="muted">Next: running the app on the emulator with a live preview arrives in the next phase.</p>
      <Nav onBack={onRestart} backLabel="Set up another project" />
    </section>
  );
}

function Nav({ onBack, onNext, nextLabel = "Next", backLabel = "Back", nextDisabled }: { onBack?: () => void; onNext?: () => void; nextLabel?: string; backLabel?: string; nextDisabled?: boolean }) {
  return (
    <div className="nav">
      {onBack ? (
        <button className="secondary" onClick={onBack}>
          {backLabel}
        </button>
      ) : (
        <span />
      )}
      {onNext && (
        <button onClick={onNext} disabled={nextDisabled}>
          {nextLabel}
        </button>
      )}
    </div>
  );
}
