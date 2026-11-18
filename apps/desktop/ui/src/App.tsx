import { useEffect, useRef, useState } from "react";
import { Stage } from "./Stage";
import { Home, useHome } from "./Home";
import { applyUpdate, call, checkUpdate, DocsPage, inTauri, loadWizard, openDocs, openInEditor, openPreview, openProblems, pickFolder, saveWizard, size, Step, UpdateInfo, WizardState } from "./api";
import { AgentsStep } from "./Agents";
import { STEP_WHY, Why } from "./why";
import { CopyAll } from "./Problems";
// the product mark, shared with the website and the app icon (brand/)
import logo from "../../../../brand/logo-mark.svg";

const STEPS: { id: Step; label: string }[] = [
  { id: "scan", label: "Scan" },
  { id: "project", label: "Project" },
  { id: "plan", label: "Plan" },
  { id: "licences", label: "Licences" },
  { id: "install", label: "Install" },
  { id: "ready", label: "Ready" },
  { id: "run", label: "Run" },
  { id: "agents", label: "Agent" },
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
  // AUD-P3-24: returning users open on the home screen; the wizard is one click away
  const home = useHome();
  const [view, setView] = useState<"home" | "wizard" | null>(null);
  // AUD-P3-38: Pocket can be switched off, and the choice is remembered on this PC
  const [pocket, setPocket] = useState(() => {
    try {
      return localStorage.getItem("umr.pocket") !== "off";
    } catch {
      return true;
    }
  });
  const togglePocket = () => {
    setPocket((on) => {
      try {
        localStorage.setItem("umr.pocket", on ? "off" : "on");
      } catch {
        /* not remembered: still switched for now */
      }
      return !on;
    });
  };
  useEffect(() => {
    if (view !== null || !state) return;
    // until the summary arrives (or with an older Core that has none) the wizard shows
    if (home) setView(home.recent.length > 0 && !state.jobId ? "home" : "wizard");
  }, [home, state, view]);

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
  const wizard = (next: Partial<WizardState>) => {
    go(next);
    setView("wizard");
  };
  const atHome = view === "home";

  return (
    <div className={pocket ? "layout" : "layout no-stage"}>
    <div className="shell">
      <header>
        <h1 className="brand">
          <img src={logo} alt="" width={30} height={30} />
          AI Mobile Runtime
        </h1>
        <p className="sub">
          {atHome ? "Your projects, the running app and your agent's work." : "Set up everything your project needs to run on the Android emulator."}
          {(home?.recent.length ?? 0) > 0 && (
            <button className="link home-toggle" onClick={() => setView(atHome ? "wizard" : "home")}>
              {atHome ? "Open setup" : "← Home"}
            </button>
          )}
        </p>
        <button className="link pocket-toggle" onClick={togglePocket} title={pocket ? "Hide the Pocket panel and its animation (remembered)" : "Show the Pocket panel again"}>
          {pocket ? "Hide Pocket" : "Show Pocket"}
        </button>
        {!inTauri && <p className="demo">Browser preview with sample data — open the desktop app for real results.</p>}
        <UpdateBanner />
      </header>
      {atHome ? (
        <main>
          <Home
            summary={home}
            onSetup={(path) => wizard({ step: "plan", projectPath: path, jobId: undefined })}
            onNewProject={() => wizard({ step: "project", projectPath: "", jobId: undefined })}
            onAgents={() => wizard({ step: "agents" })}
          />
        </main>
      ) : (
      <>
      <ol className="steps" aria-label="Setup steps">
        {STEPS.map((s, i) => (
          <li key={s.id} className={i < index ? "done" : i === index ? "current" : ""} aria-current={i === index ? "step" : undefined} title={STEP_WHY[s.id]}>
            {i < index ? (
              // UI-008: finished steps are links back; later steps cannot be skipped to
              <button type="button" className="step-back" onClick={() => go({ step: s.id, jobId: undefined })} aria-label={`Back to ${s.label}`}>
                <span className="dot">✓</span>
                {s.label}
              </button>
            ) : (
              <>
                <span className="dot">{i + 1}</span>
                {s.label}
              </>
            )}
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
        {state.step === "ready" && <ReadyStep path={state.projectPath} onRestart={() => go({ step: "project", projectPath: "", jobId: undefined })} onNext={() => go({ step: "run" })} />}
        {state.step === "run" && <RunStep path={state.projectPath} onBack={() => go({ step: "ready" })} onNext={() => go({ step: "agents" })} />}
        {state.step === "agents" && <AgentsStep project={state.projectPath} onBack={() => go({ step: "ready" })} />}
        <StepDocs step={state.step} />
      </main>
      </>
      )}
    </div>
    {pocket && <Stage step={atHome ? "home" : state.step} />}
    </div>
  );
}

/**
 * UPD-001: "a new version is available" with Update now. The update downloads and verifies the
 * installer, installs it over this copy (settings, toolchains and emulators stay) and reopens the
 * app. Checked once at start, only when an update channel is configured.
 */
function UpdateBanner() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [state, setState] = useState<"idle" | "updating" | "later">("idle");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    checkUpdate().then(setInfo).catch(() => undefined);
  }, []);
  const next = info?.available;
  if (!next || state === "later") return null;
  return (
    <div className="update" role="status">
      <span>
        <strong>Version {next.version} is available</strong> <span className="muted">· you have {info?.current}</span>
        {error && <span className="warn"> · {error}</span>}
      </span>
      <span className="update-actions">
        {state === "updating" ? (
          <span className="muted">Downloading and installing… the app closes and opens again by itself.</span>
        ) : (
          <>
            <button className="link" onClick={() => setState("later")}>Later</button>
            <button
              className="primary"
              onClick={() => {
                setState("updating");
                setError(null);
                applyUpdate().catch((e) => {
                  setError(String(e));
                  setState("idle");
                });
              }}
            >
              Update now
            </button>
          </>
        )}
      </span>
    </div>
  );
}

/** Each wizard step links to the part of the guide that explains it (9.6). */
const STEP_DOCS: Record<Step, [DocsPage, string, string]> = {
  scan: ["getting-started", "2-first-setup-the-wizard", "What the scan checks"],
  project: ["getting-started", "2-first-setup-the-wizard", "Choosing a project"],
  plan: ["getting-started", "2-first-setup-the-wizard", "What gets installed"],
  licences: ["getting-started", "2-first-setup-the-wizard", "About the licences"],
  install: ["troubleshooting", "environment", "If an install fails"],
  ready: ["getting-started", "3-your-first-app", "Running your app"],
  run: ["getting-started", "3-your-first-app", "Running your app"],
  agents: ["agents", "connect-an-agent", "Connecting coding agents"],
};

function StepDocs({ step }: { step: Step }) {
  const [page, anchor, label] = STEP_DOCS[step];
  return (
    <p className="step-docs small">
      <button className="link" onClick={() => openDocs(page, anchor).catch(() => undefined)}>
        {label} (guide) ↗
      </button>
    </p>
  );
}

const ORIGIN: Record<string, string> = {
  managed: "installed by AI Mobile Runtime",
  envvar: "named by an environment variable",
  path: "on PATH",
  registry: "Windows registry",
  commondir: "install folder",
  androidstudio: "inside Android Studio",
};
const TOOL_NAME: Record<string, string> = { flutter: "Flutter", node: "Node.js", npm: "npm", git: "Git", "android-cli": "Android CLI", yarn: "Yarn", pnpm: "pnpm", bun: "Bun" };

function origins(o: string[] | undefined): string {
  return (o ?? []).map((x) => ORIGIN[x.toLowerCase().replace(/[-_]/g, "")] ?? x).join(", ");
}

function health(h: { state: string; detail?: string } | undefined): { ok: boolean; text: string } {
  if (!h || h.state === "ok") return { ok: true, text: "works" };
  if (h.state === "broken") return { ok: false, text: `not working: ${h.detail ?? ""}` };
  return { ok: true, text: "not checked" };
}

/** AUD-P2-18: every tool the scan found, with its version, where it is and whether it runs. */
function ToolsFound({ r }: { r: any }) {
  type Row = { name: string; version: string; where: string; from: string; ok: boolean; status: string; role?: string };
  const rows: Row[] = [];
  for (const j of r.jdks ?? []) {
    const h = health(j.health);
    rows.push({ name: `${j.is_jdk ? "JDK" : "Java runtime (no JDK)"}${j.vendor ? ` · ${j.vendor}` : ""}`, version: j.version ?? (j.major ? String(j.major) : "?"), where: j.home, from: origins(j.origins), ok: h.ok && j.is_jdk, status: j.is_jdk ? h.text : "a JRE: Gradle needs a JDK" });
  }
  for (const sdk of r.android_sdks ?? []) {
    const adb = health(sdk.adb);
    const emu = health(sdk.emulator);
    const pt = (sdk.packages ?? []).find((p: any) => p.path === "platform-tools");
    // AUD-P3-39: the runtime builds and runs apps with its own SDK; any other one is left as it is
    const own = (sdk.origins ?? []).some((o: string) => o.toLowerCase() === "managed");
    const many = (r.android_sdks ?? []).length > 1;
    const role = own
      ? "builds and runs your apps"
      : many
        ? "yours (for example Android Studio's): not used and not changed"
        : "yours: not changed; the runtime installs its own SDK for builds";
    rows.push({ name: "Android SDK", version: `${(sdk.packages ?? []).length} packages${pt?.version ? ` · adb ${pt.version}` : ""}`, where: sdk.root, from: origins(sdk.origins), ok: adb.ok && emu.ok, status: `adb ${adb.text}, emulator ${emu.text}`, role });
  }
  for (const t of r.tools ?? []) {
    const h = health(t.health);
    rows.push({ name: TOOL_NAME[t.name] ?? t.name, version: t.version ?? "?", where: t.path, from: origins(t.origins), ok: h.ok, status: h.text });
  }
  if (!rows.length) return <p className="muted">No development tools found yet: the plan installs what your project needs.</p>;
  return (
    <table className="plan tools-found">
      <tbody>
        {rows.map((x, i) => (
          <tr key={i}>
            <td>
              <span className={`tag ${x.ok ? "reuse" : "install"}`}>{x.ok ? "OK" : "Check"}</span>
            </td>
            <td>
              {x.name} <span className="muted">{x.version}</span>
              {x.role && <span className={`tag ${x.role.startsWith("builds") ? "reuse" : ""} tool-role`}>{x.role}</span>}
              <div className="muted small" title={x.where}>
                {x.where && <code>{x.where}</code>}
                {[x.from, !x.ok || x.status !== "works" ? x.status : ""]
                  .filter(Boolean)
                  .map((t) => ` · ${t}`)
                  .join("")}
              </div>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
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
      <Why step="scan" />
      <div className="facts">
        <Fact label="Windows" value={`build ${r.host.windows_build}${r.host.windows_11 ? " (Windows 11)" : ""}`} ok={r.host.windows_11} />
        <Fact label="Memory" value={size(r.host.ram_total_bytes)} ok={r.host.ram_total_bytes >= 8 * 1073741824} />
        <Fact label="Free disk (toolchains)" value={size(r.host.disk_free_bytes)} ok={r.host.disk_free_bytes >= 20 * 1073741824} />
        <Fact label="Emulator acceleration" value={v.acceleration_available ? "available" : `not available (${v.whpx})`} ok={v.acceleration_available} />
        <Fact label="JDKs found" value={String(r.jdks.length)} ok />
        <Fact label="Android SDKs found" value={String(r.android_sdks.length)} ok />
      </div>
      <h3>Tools found</h3>
      <ToolsFound r={r} />
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
  const [typing, setTyping] = useState(false);
  const choose = async () => {
    const p = await pickFolder(input.trim() || undefined).catch((e) => {
      setError(String(e));
      return null;
    });
    if (p) {
      setInput(p);
      detect(p);
    }
  };
  // AUD-P3-23: a themed confirmation in the card instead of the browser's confirm() box
  const [asking, setAsking] = useState(false);
  const trust = () => {
    setAsking(false);
    call("project.trust", { path: input.trim(), trusted: true }).then(() => detect(input));
  };
  const d = result?.detection;
  const unknown = d?.framework === "unknown";
  return (
    <section>
      <h2>Your project</h2>
      <Why step="project" />
      <div className="picker">
        <button className="primary" onClick={choose} disabled={busy}>
          {input.trim() ? "Choose another folder…" : "Choose your app's folder…"}
        </button>
        {input.trim() && !typing && (
          <span className="picked" title={input}>
            📁 <code>{input}</code>
            {busy && <span className="muted"> · checking…</span>}
          </span>
        )}
      </div>
      <p className="small muted">The folder with your app's <code>pubspec.yaml</code>, <code>package.json</code> or <code>settings.gradle</code>.</p>
      {typing ? (
        <div className="row">
          <input id="path" aria-label="Project folder (full path)" value={input} placeholder="D:\Work\my-app" onChange={(e) => setInput(e.target.value)} onKeyDown={(e) => e.key === "Enter" && detect(input)} autoFocus />
          <button className="secondary" onClick={() => detect(input)} disabled={!input.trim() || busy}>
            {busy ? "Checking…" : "Check"}
          </button>
        </div>
      ) : (
        <p className="small">
          <button className="link" onClick={() => setTyping(true)}>Type a path instead</button>
        </p>
      )}
      {error && (
        // AUD-P2-19: the Core says what is wrong with the path; no "Something went wrong"
        <div className="problem" role="alert">{error.replace(/^Error:\s*/, "")}</div>
      )}
      {d && (
        <div className="card">
          {unknown ? (
            <p className="big">
              {d.candidates.length > 0 ? "No app right in this folder" : "No mobile app found here"}
            </p>
          ) : (
            <p className="big">
              {FRAMEWORK[d.framework] ?? d.framework} <span className="muted">· {d.confidence} % sure</span>
            </p>
          )}
          {unknown && d.candidates.length === 0 && (
            <p>
              Pick the folder that holds <code>pubspec.yaml</code> (Flutter), <code>package.json</code> (React Native, Expo) or{" "}
              <code>settings.gradle</code> (Android).
            </p>
          )}
          <ul className="evidence">
            {d.evidence.map((e: any, i: number) => (
              <li key={i}>
                <code>{e.file}</code> — {e.says}
              </li>
            ))}
          </ul>
          {d.candidates.length > 0 && (
            <>
              <p>{unknown ? "These folders inside it hold one; pick yours:" : "Projects inside this folder:"}</p>
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
          {!unknown && !asking && (
            <p className="muted">
              {result.trusted ? "Trusted: builds may run this project's scripts." : "Not trusted yet — you will be asked before the first build."}{" "}
              {!result.trusted && (
                <button className="link" onClick={() => setAsking(true)}>
                  Trust now
                </button>
              )}
            </p>
          )}
          {asking && <TrustPrompt onTrust={trust} onCancel={() => setAsking(false)} />}
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
      <Why step="plan" />
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
        {/* AUD-P3-34: say it plainly when there is nothing to download */}
        {installs.length === 0
          ? `Everything is already installed · free ${size(plan.disk_free_bytes)}`
          : `Download about ${size(plan.total_download_bytes)} · free ${size(plan.disk_free_bytes)}`}
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
      {(plan.notes ?? []).length > 0 && (
        <div className="note" role="note">
          <strong>Good to know</strong>
          {(plan.notes as string[]).map((n, i) => (
            <p key={i} className="small">
              {n}
            </p>
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
      <Why step="licences" />
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
      <Why step="install" />
      <InstallTotal status={status} />
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
      {status.state === "failed" && <InstallFailed status={status} onBack={onBack} />}
      {status.state === "cancelled" && <p className="muted">Cancelled. You can continue later; downloads resume.</p>}
      {status.state === "done" && <Nav onNext={onDone} nextLabel="Finish" />}
    </section>
  );
}

/** AUD-P2-20: one line for the whole install: bytes, speed over the last seconds, time left. */
function InstallTotal({ status }: { status: any }) {
  const samples = useRef<{ t: number; b: number }[]>([]);
  const items: any[] = status.items;
  const sized = items.filter((i) => i.bytes_total);
  const total = sized.reduce((a, i) => a + i.bytes_total, 0);
  const done = sized.reduce((a, i) => a + Math.min(i.bytes_done, i.bytes_total), 0);
  const finished = items.filter((i) => ["done", "already-installed", "skipped"].includes(i.state)).length;
  const now = Date.now();
  const s = samples.current;
  if (!s.length || s[s.length - 1].b !== done) s.push({ t: now, b: done });
  while (s.length > 2 && now - s[0].t > 8000) s.shift();
  const span = s.length > 1 ? (s[s.length - 1].t - s[0].t) / 1000 : 0;
  const speed = span > 1 ? (s[s.length - 1].b - s[0].b) / span : 0;
  const left = speed > 0 && total > done ? (total - done) / speed : null;
  const pct = total ? Math.round((done / total) * 100) : Math.round((finished / Math.max(1, items.length)) * 100);
  const mins = (sec: number) => (sec < 90 ? `${Math.max(1, Math.round(sec))} s` : `${Math.round(sec / 60)} min`);
  return (
    <div className="install-total">
      <div className="row">
        <strong>
          {finished} of {items.length} done
          {total > 0 && ` · ${size(done)} of ${size(total)}`}
        </strong>
        <span className="muted">
          {status.state === "running" && speed > 0 && `${size(speed)}/s`}
          {status.state === "running" && left !== null && ` · about ${mins(left)} left`}
        </span>
      </div>
      <div className="bar big" role="progressbar" aria-label="Whole install" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <div style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

/** AUD-P2-20: a failed install says what failed and offers the log and a copy of the error. */
function InstallFailed({ status, onBack }: { status: any; onBack: () => void }) {
  const [note, setNote] = useState<string | null>(null);
  const failed = (status.items as any[]).filter((i) => i.state === "failed");
  const text = [
    `Install failed: ${status.message ?? "unknown error"}`,
    ...failed.map((i) => `- ${i.title}${i.version ? ` ${i.version}` : ""}: ${i.message ?? "failed"}`),
    status.log ? `Log: ${status.log}` : "",
  ]
    .filter(Boolean)
    .join("\n");
  return (
    <div className="problem" role="alert">
      <div>
        <strong>The install stopped.</strong> {status.message ?? ""}
        {failed.map((i) => (
          <div key={i.component + (i.package ?? "")} className="small">
            {i.title}: {i.message ?? "failed"}
          </div>
        ))}
        {note && <div className="small">{note}</div>}
      </div>
      <span>
        {status.log && (
          <button className="secondary" onClick={() => openInEditor(status.log).then((w) => setNote(`Opened in ${w}.`), (e) => setNote(String(e)))}>
            Open log
          </button>
        )}{" "}
        <button className="secondary" onClick={() => navigator.clipboard.writeText(text).then(() => setNote("Copied."), () => setNote("Could not copy."))}>
          Copy error
        </button>{" "}
        <button className="secondary" onClick={onBack}>Try again</button>
      </span>
    </div>
  );
}

/** AUD-P2-22: where the disk space goes, largest first, in plain words. */
const DISK_GROUPS: [RegExp, string, string][] = [
  [/^android\/system-images\//, "Emulator system images", "the Android the emulator runs, one per API level"],
  [/^android\/ndk\//, "Android NDK", "native C/C++ code; React Native and Flutter plugins need it"],
  [/^android\/emulator$/, "Android emulator", "the program that runs the phone"],
  [/^android\/cmake\//, "CMake", "builds native code"],
  [/^android\//, "Android SDK", "platforms, build tools, adb"],
  [/^jdk\//, "Java (JDK)", "runs Gradle"],
  [/^flutter@/, "Flutter SDK", "with its Dart SDK and engine files"],
  [/^node/, "Node.js", "runs Metro for React Native"],
];
function DiskBreakdown({ components, disk }: { components: any[]; disk: any }) {
  const rows = new Map<string, { bytes: number; why: string }>();
  const add = (name: string, why: string, bytes: number) => {
    if (bytes <= 0) return;
    const r = rows.get(name) ?? { bytes: 0, why };
    r.bytes += bytes;
    rows.set(name, r);
  };
  for (const c of components) {
    if (c.source !== "managed" || c.kind === "android-cli-runtime") continue;
    const g = DISK_GROUPS.find(([re]) => re.test(c.id));
    add(g ? g[1] : c.kind, g ? g[2] : c.id, c.size_bytes);
  }
  add("Emulators", "your virtual phones with their apps, data and quick-start snapshots", disk.android_user_home_bytes);
  add("Download cache", "installers kept to resume or repair; safe to clean", disk.download_cache_bytes);
  add("Temporary files and logs", "build and install leftovers", disk.temp_bytes + disk.logs_bytes + disk.staging_bytes);
  const list = [...rows.entries()].sort((a, b) => b[1].bytes - a[1].bytes);
  if (!list.length) return null;
  return (
    <details className="disk">
      <summary>Where the {size(disk.total_bytes)} go</summary>
      <table>
        <tbody>
          {list.map(([name, r]) => (
            <tr key={name}>
              <td className="num">{size(r.bytes)}</td>
              <td>
                <strong>{name}</strong> <span className="muted small">{r.why}</span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted small">
        Only what the runtime installed is counted; tools you had before stay where they are. <code>umr components</code> lists every item;
        removing a project type's tools frees its share.
      </p>
    </details>
  );
}

function ReadyStep({ path, onRestart, onNext }: { path: string; onRestart: () => void; onNext: () => void }) {
  const { data, error, loading, reload } = useAsync(() => call("components.list"), []);
  if (loading) return <Busy text="Checking the installed tools…" />;
  if (error) return <Problem text={error} retry={reload} />;
  const r: any = data;
  const broken = r.components.filter((c: any) => !c.ok);
  return (
    <section>
      <h2>{broken.length ? "Almost ready" : "Ready"}</h2>
      <Why step="ready" />
      <p>
        {r.components.length} components installed and checked ({size(r.disk.total_bytes)}).
        {broken.length > 0 && <strong className="warn"> {broken.length} need repair — run setup again.</strong>}
      </p>
      <DiskBreakdown components={r.components} disk={r.disk} />
      <p>
        Project: <code>{path}</code>
      </p>
      <Nav onBack={onRestart} backLabel="Set up another project" onNext={onNext} nextLabel="Run your app" />
    </section>
  );
}

/** Step "Run": the first run of the project on the emulator, then the live preview (9.6). */
const RUN_STAGES: { id: string; label: string; states: string[] }[] = [
  { id: "device", label: "Start the emulator", states: ["preparing", "env_blocked", "booting"] },
  { id: "build", label: "Build the app", states: ["building"] },
  { id: "launch", label: "Install and start it", states: ["launching"] },
];

/**
 * AUD-P1-6: what a run is doing while the Run step waits (first builds take minutes): the
 * stage from the session, the time so far, the latest build lines, and Cancel.
 */
function RunProgress({ onCancel }: { onCancel: () => void }) {
  const [state, setState] = useState("");
  const [lines, setLines] = useState<string[]>([]);
  const [seconds, setSeconds] = useState(0);
  const [cancelling, setCancelling] = useState(false);
  useEffect(() => {
    const t0 = Date.now();
    let stop = false;
    const tick = async () => {
      setSeconds(Math.round((Date.now() - t0) / 1000));
      try {
        const s = await call<{ session?: { state?: string } | null }>("session.status");
        if (!stop) setState(s.session?.state ?? "");
        const l = await call<{ lines: { source: string; text: string }[] }>("logs.tail", { limit: 40, level: "I" });
        if (!stop)
          setLines(
            l.lines
              .filter((x) => x.source !== "logcat" && x.text.trim())
              .slice(-4)
              .map((x) => x.text.slice(0, 160)),
          );
      } catch {
        /* the Core is busy starting: next tick */
      }
    };
    tick();
    const id = window.setInterval(tick, 1000);
    return () => {
      stop = true;
      window.clearInterval(id);
    };
  }, []);
  const current = RUN_STAGES.findIndex((st) => st.states.includes(state));
  const mm = Math.floor(seconds / 60);
  const ss = String(seconds % 60).padStart(2, "0");
  return (
    <div className="card run-progress" role="status">
      <ol className="run-stages">
        {RUN_STAGES.map((st, i) => (
          <li key={st.id} className={i < current ? "done" : i === current ? "current" : ""}>
            <span className="dot">{i < current ? "✓" : i + 1}</span>
            {st.label}
          </li>
        ))}
      </ol>
      <p className="small muted">
        {mm}:{ss} so far · the first build of a project can take several minutes
      </p>
      {lines.length > 0 && <pre className="run-lines">{lines.join("\n")}</pre>}
      <button
        className="secondary"
        disabled={cancelling}
        onClick={() => {
          setCancelling(true);
          onCancel();
        }}
      >
        {cancelling ? "Cancelling…" : "Cancel"}
      </button>
    </div>
  );
}

/** AUD-P3-23: the themed "trust this folder?" question (Project step, and Run for an untrusted folder). */
function TrustPrompt({ onTrust, onCancel }: { onTrust: () => void; onCancel: () => void }) {
  return (
    <div className="confirm" role="alertdialog" aria-label="Trust this folder?">
      <p>
        <strong>Trust this folder?</strong> Building runs the project's own code: Gradle scripts, npm install scripts and Flutter
        plugins. Trust only projects you know.
      </p>
      <div className="row">
        <span />
        <span>
          <button className="secondary" onClick={onCancel}>Not now</button>{" "}
          <button onClick={onTrust} autoFocus>Trust this folder</button>
        </span>
      </div>
    </div>
  );
}

function RunStep({ path, onBack, onNext }: { path: string; onBack: () => void; onNext: () => void }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<any>(null);
  const [error, setError] = useState<string | null>(null);
  const run = () => {
    setBusy(true);
    setError(null);
    setResult(null);
    call("project.run", { path, watch: true })
      .then((v) => {
        setResult(v);
        if (v?.status === "running") openPreview().catch(() => undefined);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  const ok = result?.status === "running";
  return (
    <section>
      <h2>Run your app</h2>
      <Why step="run" />
      <p>
        The runtime starts the emulator, builds <code>{path || "your project"}</code>, installs it and opens the live preview beside
        your other windows. Saved files reload by themselves.
      </p>
      <p className="muted small">The first build of a project can take several minutes; later runs take seconds.</p>
      {!ok && !busy && (
        <button onClick={run} disabled={!path}>
          Run the app
        </button>
      )}
      {busy && <RunProgress onCancel={() => call("project.stop").catch(() => undefined)} />}
      {!ok && result?.status === "cancelled" && <p className="muted">Cancelled. Run it again whenever you like.</p>}
      {error && /not trusted yet/.test(error) ? (
        <TrustPrompt onTrust={() => call("project.trust", { path, trusted: true }).then(run, (e) => setError(String(e)))} onCancel={() => setError(null)} />
      ) : (
        error && <Problem text={error} retry={run} />
      )}
      {result && !ok && result.status !== "cancelled" && (
        <div className="problem" role="alert">
          <div>
            <strong>The app did not start.</strong> {result.error ?? result.status}
            {(result.problems ?? []).slice(0, 3).map((p: any, i: number) => (
              <div key={i} className="small">
                {p.category}: {p.message}
                {p.file ? ` (${p.file}${p.line ? `:${p.line}` : ""})` : ""}
              </div>
            ))}
            {(result.problems ?? []).length > 3 && <div className="small">…and {(result.problems ?? []).length - 3} more.</div>}
          </div>
          {/* AUD-P2-26: every problem, with its code and stack, in the preview's panel or on the clipboard */}
          <span className="run-fail-actions">
            <button className="secondary" onClick={() => openProblems().catch(() => undefined)}>Open the Problems panel</button>{" "}
            <CopyAll fetch />
          </span>
        </div>
      )}
      {ok && (
        <div className="card">
          <p className="big">
            <span className="ok">Running</span> {result.package} on {result.serial}
          </p>
          <p className="small muted">
            {/* AUD-P2-27: the whole wait, split into the device and the app */}
            {result.adapter} adapter · {result.seconds} s
            {result.device_seconds != null && ` (emulator ${result.device_seconds} s, build and launch ${result.app_seconds} s)`}
            {result.watching ? " · reloads when you save" : ""}
          </p>
          <button className="secondary" onClick={() => openPreview().catch(() => undefined)}>
            Open the live preview
          </button>
        </div>
      )}
      <p className="small muted">
        From a terminal the same is <code>umr run</code> in the project folder.
      </p>
      <Nav onBack={onBack} onNext={onNext} nextLabel={ok ? "Connect your agent" : "Skip to your agent"} />
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
