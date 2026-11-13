// Phase 4 preview window: frames from the Core's preview server drawn with WebGL2 (PRV-009),
// pointer / wheel / keyboard input sent back (PRV-010, PRV-004), toolbar (PRV-005) and device
// switcher (PRV-007). The window reports its own timing so the Core can check NFR-02a / NFR-03.
import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "./api";
import { FrameRenderer, percentiles } from "./gl";
import { Approvals, DiagnosticsPanel, useProblems } from "./Problems";

interface Hello {
  type: "hello";
  serial: string;
  device_width: number;
  device_height: number;
  width: number;
  height: number;
  interact: boolean;
}

interface Profile {
  profile: { id: string; class: string; fold?: { kind: string } | null };
}

const FRAME_HEADER = 21;
const SPECIAL_KEYS = new Set([
  "Enter", "Backspace", "Delete", "Tab", "Escape", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
  "Home", "End", "PageUp", "PageDown",
]);

export function Preview() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const rendererRef = useRef<FrameRenderer | null>(null);
  const helloRef = useRef<Hello | null>(null);
  const frameSize = useRef<{ w: number; h: number }>({ w: 0, h: 0 });
  const draws = useRef<number[]>([]);
  const hops = useRef<number[]>([]);
  const frameTimes = useRef<number[]>([]);
  const framesTotal = useRef(0);
  const pending = useRef<{ data: Uint8Array; w: number; h: number } | null>(null);
  const down = useRef(new Set<number>());

  const [status, setStatus] = useState("Connecting to the Runtime Core…");
  const [device, setDevice] = useState<Hello | null>(null);
  const [profile, setProfile] = useState<string>("");
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [fps, setFps] = useState(0);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [panel, setPanel] = useState(false);
  const problems = useProblems();
  const errors = problems.filter((p) => p.severity === "error").length;

  const send = (m: unknown) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(m));
  };

  // draw the newest frame once per display refresh (frames that arrive in between are skipped)
  const drawLoop = useCallback(() => {
    const f = pending.current;
    const r = rendererRef.current;
    if (f && r) {
      pending.current = null;
      draws.current.push(r.draw(f.data, f.w, f.h));
      if (draws.current.length > 600) draws.current.shift();
      const now = performance.now();
      frameTimes.current.push(now);
      while (frameTimes.current.length && now - frameTimes.current[0] > 3000) frameTimes.current.shift();
      framesTotal.current++;
    }
    requestAnimationFrame(drawLoop);
  }, []);

  const sendResize = useCallback(() => {
    const c = canvasRef.current;
    if (!c) return;
    // at most 1.5 device pixels per CSS pixel: sharp enough, and the emulator reads back far
    // fewer pixels per frame (POC-013: ~540x1200 streams fastest)
    const scale = Math.min(window.devicePixelRatio || 1, 1.5);
    c.width = Math.max(64, Math.round(c.clientWidth * scale));
    c.height = Math.max(64, Math.round(c.clientHeight * scale));
    send({ t: "resize", w: c.width, h: c.height });
  }, []);

  const connect = useCallback(async () => {
    try {
      const r = await call<{ url: string; profile: string | null }>("preview.open", { interact: true });
      if (r.profile) setProfile(r.profile);
      if (!r.url) {
        setStatus("Preview is only available in the desktop app.");
        return;
      }
      const ws = new WebSocket(r.url);
      ws.binaryType = "arraybuffer";
      wsRef.current = ws;
      ws.onopen = () => sendResize();
      ws.onmessage = (ev) => {
        if (typeof ev.data === "string") {
          const m = JSON.parse(ev.data);
          if (m.type === "hello") {
            helloRef.current = m;
            setDevice(m);
            setStatus("");
          } else if (m.type === "waiting") {
            setDevice(null);
            setStatus("No device is running. Start one with `umr run` or pick a device above.");
          } else if (m.type === "error") {
            setStatus(`Preview error: ${m.message}`);
          }
          return;
        }
        const buf = ev.data as ArrayBuffer;
        if (buf.byteLength < FRAME_HEADER) return;
        const dv = new DataView(buf);
        const w = dv.getUint32(1, true);
        const h = dv.getUint32(5, true);
        const sentUs = Number(dv.getBigUint64(13, true));
        hops.current.push(Math.max(0, Date.now() - sentUs / 1000));
        if (hops.current.length > 600) hops.current.shift();
        frameSize.current = { w, h };
        pending.current = { data: new Uint8Array(buf, FRAME_HEADER), w, h };
      };
      ws.onclose = () => {
        wsRef.current = null;
        setStatus("Reconnecting…");
        setTimeout(connect, 1000);
      };
    } catch (e) {
      setStatus(`Cannot reach the Runtime Core: ${e}`);
      setTimeout(connect, 2000);
    }
  }, [sendResize]);

  useEffect(() => {
    const c = canvasRef.current!;
    try {
      rendererRef.current = new FrameRenderer(c);
    } catch (e) {
      setStatus(String(e));
      return;
    }
    requestAnimationFrame(drawLoop);
    const ro = new ResizeObserver(() => sendResize());
    ro.observe(c);
    connect();
    call<{ profiles: Profile[] }>("device.profiles").then((v) => setProfiles(v.profiles ?? [])).catch(() => {});
    // the window's timing → Core (NFR-02a / NFR-03), and the fps badge
    const timer = setInterval(() => {
      const d = percentiles(draws.current);
      const hop = percentiles(hops.current);
      const f = frameTimes.current.length / 3;
      setFps(Math.round(f));
      send({ t: "stats", draw_p50: d.p50, draw_p95: d.p95, hop_p95: hop.p95, fps: f, frames: framesTotal.current });
    }, 3000);
    return () => {
      clearInterval(timer);
      ro.disconnect();
      wsRef.current?.close();
    };
  }, [connect, drawLoop, sendResize]);

  /** Pointer position → device pixels (rotation-aware: frames come in the screen's orientation). */
  const toDevice = (e: { clientX: number; clientY: number }): { x: number; y: number } | null => {
    const c = canvasRef.current;
    const r = rendererRef.current;
    const hello = helloRef.current;
    const { w: fw, h: fh } = frameSize.current;
    if (!c || !r || !hello || !fw) return null;
    const rect = c.getBoundingClientRect();
    const px = ((e.clientX - rect.left) * c.width) / rect.width;
    const py = ((e.clientY - rect.top) * c.height) / rect.height;
    const cr = r.contentRect(fw, fh);
    const nx = (px - cr.x) / cr.w;
    const ny = (py - cr.y) / cr.h;
    if (nx < 0 || ny < 0 || nx > 1 || ny > 1) return null;
    const landscape = fw > fh;
    const portraitDevice = hello.device_width < hello.device_height;
    const [dw, dh] = landscape === !portraitDevice ? [hello.device_width, hello.device_height] : [hello.device_height, hello.device_width];
    return { x: Math.round(nx * dw), y: Math.round(ny * dh) };
  };

  const onPointer = (kind: "down" | "move" | "up") => (e: React.PointerEvent<HTMLCanvasElement>) => {
    const id = e.pointerId % 10;
    if (kind === "down") {
      e.currentTarget.setPointerCapture(e.pointerId);
      e.currentTarget.focus();
      down.current.add(id);
    } else if (!down.current.has(id)) {
      return;
    }
    const p = toDevice(e);
    if (kind === "up") down.current.delete(id);
    if (!p) return;
    send({ t: "touch", x: p.x, y: p.y, p: kind === "up" ? 0 : 1, id });
  };

  const onWheel = (e: React.WheelEvent) => {
    send({ t: "wheel", dx: 0, dy: e.deltaY > 0 ? -120 : 120 });
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key.length === 1) send({ t: "text", text: e.key });
    else if (SPECIAL_KEYS.has(e.key)) send({ t: "key", key: e.key, e: "press" });
    else return;
    e.preventDefault();
  };

  const act = async (label: string, f: () => Promise<unknown>) => {
    setBusy(true);
    setNote(`${label}…`);
    try {
      const r = await f();
      setNote(typeof r === "string" ? r : `${label}: done`);
    } catch (e) {
      setNote(`${label}: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const foldable = profiles.find((p) => p.profile.id === profile)?.profile.fold;

  return (
    <div className="pv">
      <div className="pv-bar">
        <select
          className="pv-select"
          value={profile}
          disabled={busy}
          title="Device"
          onChange={(e) => {
            const id = e.target.value;
            setProfile(id);
            act(`Switching to ${id}`, () => call("session.start", { profile: id }));
          }}
        >
          {!profile && <option value="">device…</option>}
          {profiles.map((p) => (
            <option key={p.profile.id} value={p.profile.id}>{p.profile.id}</option>
          ))}
        </select>
        <button title="Back" onClick={() => send({ t: "key", key: "GoBack" })}>◀</button>
        <button title="Home" onClick={() => send({ t: "key", key: "GoHome" })}>●</button>
        <button title="Recent apps" onClick={() => send({ t: "key", key: "AppSwitch" })}>■</button>
        <button title="Rotate" disabled={busy} onClick={() => act("Rotate", () => call("device.rotate", {}))}>⟲</button>
        {foldable && (
          <select
            className="pv-select"
            title="Fold"
            defaultValue=""
            onChange={(e) => {
              const v = e.target.value;
              if (v) act(`Posture ${v}`, () => call("device.posture", { posture: v }));
              e.target.value = "";
            }}
          >
            <option value="">fold…</option>
            <option value="opened">open</option>
            <option value="half-opened">half</option>
            <option value="closed">closed</option>
          </select>
        )}
        <button title="Screenshot" disabled={busy} onClick={() => act("Screenshot", async () => {
          const r = await call<{ path: string }>("device.screenshot", {});
          return `Saved ${r.path}`;
        })}>⧉</button>
        <button title="Reload the app" disabled={busy} onClick={() => act("Reload", () => call("project.reload", { kind: "hot" }))}>↻</button>
        <button title="Latency self-test" disabled={busy} onClick={() => act("Self-test", async () => {
          const r = await call<any>("preview.selftest", { taps: 20 });
          const v = r.verdict;
          return `touch→visible p50 ${v.nfr_02b.p50_ms} / p95 ${v.nfr_02b.p95_ms} ms · ${v.nfr_03.fps} fps · ours ${v.nfr_02a.pipeline_p95_ms} ms${v.host_overloaded ? " · host overloaded" : ""}`;
        })}>⏱</button>
        <button
          className={errors ? "pv-problems has" : "pv-problems"}
          title="Problems and logs of the current run"
          onClick={() => setPanel(!panel)}
        >{errors ? `⚠ ${errors}` : "☰"}</button>
        <span className="pv-fps" title="Frames drawn per second">{device ? `${fps} fps` : ""}</span>
      </div>
      <Approvals />
      <div className="pv-stage">
        <canvas
          ref={canvasRef}
          className="pv-canvas"
          tabIndex={0}
          onPointerDown={onPointer("down")}
          onPointerMove={onPointer("move")}
          onPointerUp={onPointer("up")}
          onPointerCancel={onPointer("up")}
          onWheel={onWheel}
          onKeyDown={onKey}
          onContextMenu={(e) => e.preventDefault()}
        />
        {status && <div className="pv-overlay">{status}</div>}
      </div>
      {panel && <DiagnosticsPanel problems={problems} onClose={() => setPanel(false)} />}
      {note && <div className="pv-note" onClick={() => setNote("")}>{note}</div>}
    </div>
  );
}
