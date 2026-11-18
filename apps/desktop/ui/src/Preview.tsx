// Phase 4 preview window: frames from the Core's preview server drawn with WebGL2 (PRV-009),
// pointer / wheel / keyboard input sent back (PRV-010, PRV-004), toolbar (PRV-005) and device
// switcher (PRV-007). The window reports its own timing so the Core can check NFR-02a / NFR-03.
// PRV-016: the screen sits in a phone body (thin bezel, punch-hole camera) and shows an
// in-display fingerprint sensor while the lock screen or an app's biometric prompt waits for one.
import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowLeft, Camera, FolderOpen, Gauge, House, Layers, RefreshCw, RotateCwSquare, ScrollText, Square, TriangleAlert } from "lucide-react";
import { call, inApp, inTauri, setWindowTitle, widenWindow } from "./api";
import { FrameRenderer, percentiles } from "./gl";
import { Approvals, DiagnosticsPanel, RunBanner, useProblems } from "./Problems";

interface Hello {
  type: "hello";
  serial: string;
  device_width: number;
  device_height: number;
  width: number;
  height: number;
  interact: boolean;
}

/** `device.auth_state` (PRV-016). */
interface AuthState {
  locked: boolean;
  sensor_active: boolean;
  enrolled: number;
  lock_disabled?: boolean;
}

interface Profile {
  profile: { id: string; class: string; fold?: { kind: string } | null };
  /** AUD-P1-45: switching to it starts from a saved snapshot (seconds), not a first long start. */
  warm?: boolean | null;
  preparing?: boolean;
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
  // AUD-P1-44: the moving half of a bent foldable is a copy of that half of the screen
  const lastFrame = useRef<{ data: Uint8Array; w: number; h: number } | null>(null);
  // the two halves of a bent foldable: left/right (book) or top/bottom (flip)
  const halfARef = useRef<HTMLCanvasElement>(null);
  const halfBRef = useRef<HTMLCanvasElement>(null);
  const bentRef = useRef<"book" | "flip" | null>(null);
  const poseRef = useRef<{ kind: "book" | "flip"; a: number; b: number } | null>(null);
  const down = useRef(new Set<number>());

  const [status, setStatus] = useState("Connecting to the Runtime Core…");
  const [device, setDevice] = useState<Hello | null>(null);
  const [profile, setProfile] = useState<string>("");
  const [profiles, setProfiles] = useState<Profile[]>([]);
  // AUD-P1-13: frames per second while the screen changes; null while it is still (the emulator
  // sends no frames then, and "0 fps" read like a broken stream)
  const [fps, setFps] = useState<number | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [panel, setPanel] = useState(false);
  // AUD-P2-26: "Open the Problems panel" in the setup window (a fresh request, under 30 s old)
  useEffect(() => {
    const check = () => {
      try {
        const t = Number(localStorage.getItem("umr.openProblems") ?? 0);
        if (t && Date.now() - t < 30_000) {
          localStorage.removeItem("umr.openProblems");
          setPanel(true);
        }
      } catch {
        /* no shared storage: the user opens the panel */
      }
    };
    check();
    const onStorage = (e: StorageEvent) => e.key === "umr.openProblems" && check();
    window.addEventListener("storage", onStorage);
    window.addEventListener("focus", check);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("focus", check);
    };
  }, []);
  // AUD-P2-21: the panel sits beside the phone; the window grows by its width and gives it back
  const added = useRef(0);
  useEffect(() => {
    if (panel && added.current === 0) widenWindow(380).then((px) => (added.current = px), () => undefined);
    if (!panel && added.current > 0) {
      const px = added.current;
      added.current = 0;
      widenWindow(-px).catch(() => undefined);
    }
  }, [panel]);
  const [focus, setFocus] = useState<string | undefined>(undefined);
  const stageRef = useRef<HTMLDivElement>(null);
  const [stage, setStage] = useState({ w: 0, h: 0 });
  const [landscape, setLandscape] = useState(false);
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [finger, setFinger] = useState(false);
  const problems = useProblems();
  // AUD-P1-5: "Phone — rncalc" in the taskbar, not the setup window's name
  useEffect(() => {
    let last = "";
    const tick = () =>
      call<{ session?: { app?: string | null; profile?: string; state?: string } | null }>("session.status")
        .then((s) => {
          // the device may be switched from elsewhere (CLI, agent, setup window): follow it once
          // the switch has finished, so the menus (hinge control) match the device on screen
          const p = s.session?.profile;
          if (p && s.session?.state === "running") setProfile((cur) => (cur === p ? cur : p));
          const app = s.session?.app?.split(".").pop();
          const t = app ? `Phone — ${app}` : "Phone — AI Mobile Runtime";
          if (t !== last) {
            last = t;
            setWindowTitle(t);
          }
        })
        .catch(() => undefined);
    tick();
    const id = window.setInterval(tick, 4000);
    return () => window.clearInterval(id);
  }, []);
  const errors = problems.filter((p) => p.severity === "error").length;

  const send = (m: unknown) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(m));
  };

  /** AUD-P1-44: copies each half of the screen into the leaf of the device that carries it (left
   * and right half of a book fold, top and bottom half of a flip); right after a draw, while the
   * frame is in the canvas. The halves meet exactly at the hinge: one continuous inner display. */
  const mirrorHalf = () => {
    const c = canvasRef.current;
    const kind = bentRef.current;
    if (!c || !kind || !c.width) return;
    const cut = kind === "book" ? Math.round(c.width / 2) : Math.round(c.height / 2);
    const parts: [HTMLCanvasElement | null, number, number, number, number][] =
      kind === "book"
        ? [[halfARef.current, 0, 0, cut, c.height], [halfBRef.current, cut, 0, c.width - cut, c.height]]
        : [[halfARef.current, 0, 0, c.width, cut], [halfBRef.current, 0, cut, c.width, c.height - cut]];
    for (const [h, sx, sy, sw, sh] of parts) {
      if (!h || sw < 1 || sh < 1) continue;
      if (h.width !== sw) h.width = sw;
      if (h.height !== sh) h.height = sh;
      h.getContext("2d")?.drawImage(c, sx, sy, sw, sh, 0, 0, sw, sh);
    }
  };

  // draw the newest frame once per display refresh (frames that arrive in between are skipped)
  const drawLoop = useCallback(() => {
    const f = pending.current;
    const r = rendererRef.current;
    if (f && r) {
      pending.current = null;
      lastFrame.current = f;
      draws.current.push(r.draw(f.data, f.w, f.h));
      if (bentRef.current) mirrorHalf();
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
          } else if (m.type === "note") {
            setNote(m.message);
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
        setLandscape(w > h);
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
    const so = new ResizeObserver(([e]) => setStage({ w: e.contentRect.width, h: e.contentRect.height }));
    if (stageRef.current) so.observe(stageRef.current);
    connect();
    call<{ profiles: Profile[] }>("device.profiles").then((v) => setProfiles(v.profiles ?? [])).catch(() => {});
    // the window's timing → Core (NFR-02a / NFR-03), and the fps badge
    const timer = setInterval(() => {
      const d = percentiles(draws.current);
      const hop = percentiles(hops.current);
      const now = performance.now();
      const recent = frameTimes.current.filter((t) => now - t <= 3000);
      const span = recent.length > 1 ? (recent[recent.length - 1] - recent[0]) / 1000 : 0;
      const f = recent.length >= 5 && span > 0.2 ? Math.min(120, (recent.length - 1) / span) : 0;
      setFps(f > 0 ? Math.round(f) : null);
      send({ t: "stats", draw_p50: d.p50, draw_p95: d.p95, hop_p95: hop.p95, fps: f, frames: framesTotal.current });
    }, 3000);
    return () => {
      clearInterval(timer);
      ro.disconnect();
      so.disconnect();
      wsRef.current?.close();
    };
  }, [connect, drawLoop, sendResize]);

  // PRV-016: is the lock screen up, or is an app (or Settings) waiting for a fingerprint?
  useEffect(() => {
    if (!device && inApp) {
      setAuth(null);
      return;
    }
    let busy = false;
    const poll = () => {
      if (busy || document.hidden) return;
      busy = true;
      call<AuthState>("device.auth_state", {})
        .then(setAuth)
        .catch(() => setAuth(null))
        .finally(() => (busy = false));
    };
    poll();
    const t = setInterval(poll, 1000);
    return () => clearInterval(t);
  }, [device]);

  const onFinger = (down: boolean) => (e: React.PointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    e.preventDefault();
    if (down) e.currentTarget.setPointerCapture(e.pointerId);
    else if (!finger) return;
    setFinger(down);
    send({ t: "finger", down, id: 1 });
  };

  /** Pointer position → device pixels (rotation-aware: frames come in the screen's orientation). */
  const toDevice = (e: { clientX: number; clientY: number }): { x: number; y: number } | null => {
    const c = canvasRef.current;
    const r = rendererRef.current;
    const hello = helloRef.current;
    const { w: fw, h: fh } = frameSize.current;
    if (!c || !r || !hello || !fw) return null;
    const rect = c.getBoundingClientRect();
    let { clientX, clientY } = e;
    // AUD-P1-44: on a bent foldable a point on a turned half is foreshortened by the cosine of its
    // turn: undo that (the perspective is weak, so this lands on the right control)
    const pose = poseRef.current;
    if (pose?.kind === "book") {
      const hx = rect.left + rect.width / 2;
      const turn = clientX < hx ? pose.a : pose.b;
      const cos = Math.cos((turn * Math.PI) / 180);
      if (cos < 0.2) return null;
      clientX = hx + (clientX - hx) / cos;
    } else if (pose?.kind === "flip") {
      const hy = rect.top + rect.height / 2;
      const turn = clientY < hy ? pose.a : pose.b;
      const cos = Math.cos((turn * Math.PI) / 180);
      if (cos < 0.2) return null;
      clientY = hy + (clientY - hy) / cos;
    }
    const px = ((clientX - rect.left) * c.width) / rect.width;
    const py = ((clientY - rect.top) * c.height) / rect.height;
    const cr = r.contentRect(fw, fh);
    const nx = (px - cr.x) / cr.w;
    const ny = (py - cr.y) / cr.h;
    if (nx < 0 || ny < 0 || nx > 1 || ny > 1) return null;
    const landscape = fw > fh;
    const portraitDevice = hello.device_width < hello.device_height;
    const [dw, dh] = landscape === !portraitDevice ? [hello.device_width, hello.device_height] : [hello.device_height, hello.device_width];
    return { x: Math.round(nx * dw), y: Math.round(ny * dh) };
  };

  const onPointer = (kind: "down" | "move" | "up") => (e: React.PointerEvent<HTMLElement>) => {
    const id = e.pointerId % 10;
    if (kind === "down") {
      e.currentTarget.setPointerCapture(e.pointerId);
      canvasRef.current?.focus();
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

  // AUD-P1-45: what a device switch is doing now, with the time so far
  const [switching, setSwitching] = useState<{ to: string; t0: number; cold: boolean } | null>(null);
  const [switchStage, setSwitchStage] = useState("");
  useEffect(() => {
    if (!switching) {
      setSwitchStage("");
      return;
    }
    const words: Record<string, string> = {
      idle: "stopping the previous device",
      preparing: "getting ready",
      booting: switching.cold ? "starting the emulator for the first time (about 2 minutes, once)" : "starting the emulator",
      building: "installing your app",
      launching: "starting your app",
      running: "done",
    };
    let stop = false;
    const tick = async () => {
      const s = await call<{ session?: { state?: string; profile?: string } | null }>("session.status").catch(() => null);
      const st = s?.session?.profile === switching.to ? s?.session?.state ?? "" : "idle";
      const secs = Math.round((Date.now() - switching.t0) / 1000);
      if (!stop) setSwitchStage(`Switching to ${switching.to}: ${words[st] ?? st} · ${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`);
    };
    tick();
    const t = window.setInterval(tick, 1000);
    return () => {
      stop = true;
      window.clearInterval(t);
    };
  }, [switching]);
  /** AUD-P1-45: boots each device that has no snapshot yet, one after the other, in the background. */
  const prepareOthers = async () => {
    const todo = profiles.filter((p) => p.warm === false && !p.preparing && p.profile.id !== profile).map((p) => p.profile.id);
    setNote(`Getting ${todo.join(", ")} ready in the background (about 2 minutes each); you can keep working`);
    for (const id of todo) {
      await call("device.prepare", { profile: id }).catch(() => undefined);
      for (let i = 0; i < 120; i++) {
        await new Promise((r) => setTimeout(r, 5000));
        const v = await call<{ profiles: Profile[] }>("device.profiles").catch(() => null);
        if (v) setProfiles(v.profiles ?? []);
        if (v && !v.profiles.find((p) => p.profile.id === id)?.preparing) break;
      }
    }
    setNote("The other devices are ready: switching to them takes seconds now");
  };

  // AUD-P1-40/41: the hinge as the Core reports it (also when an agent or the CLI moves it)
  const [fold, setFold] = useState<FoldInfo | null>(null);
  useEffect(() => {
    // a new device: forget the previous one's hinge (fold → flip both start at 180°)
    setFold(null);
    if (!foldable) return;
    let stop = false;
    const tick = () =>
      call<FoldInfo>("device.posture", {}).then(
        (f) =>
          !stop &&
          setFold((old) =>
            old && old.kind === f.kind && old.angle === f.angle && old.cover?.width === f.cover?.width ? old : f,
          ),
        () => undefined,
      );
    tick();
    const t = window.setInterval(tick, 2500);
    return () => {
      stop = true;
      window.clearInterval(t);
    };
  }, [profile, !!foldable, device?.serial]); // eslint-disable-line react-hooks/exhaustive-deps
  const setAngle = (a: number) => {
    // the halves start moving at once; the Core's answer settles posture and cover screen
    setFold((f) => (f ? { ...f, angle: a, cover: a > 30 ? null : f.cover } : f));
    act(`Hinge ${a}°`, async () => {
      const f = await call<FoldInfo>("device.posture", { angle: a });
      setFold(f);
      return f.cover ? `${f.label}: ${f.cover.label}` : f.label;
    });
  };
  // AUD-P1-44: the angle drawn, eased toward the hinge's angle
  const [shown, setShown] = useState(180);
  const shownRef = useRef(180);
  useEffect(() => {
    const target = fold?.angle ?? 180;
    const from = shownRef.current;
    if (Math.abs(from - target) < 0.1) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      shownRef.current = target;
      setShown(target);
      return;
    }
    const t0 = performance.now();
    const dur = 260 + Math.abs(target - from) * 2;
    let raf = 0;
    const step = (now: number) => {
      const k = Math.min(1, (now - t0) / dur);
      const e = k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2;
      const v = from + (target - from) * e;
      shownRef.current = v;
      setShown(v);
      if (k < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [fold?.angle]);
  const kind = fold?.kind === "flip" ? "flip" : fold ? "book" : null;
  const hinge = kind && !landscape && !fold?.cover ? kind : null;
  const bent = hinge && shown < 179.5 ? hinge : null;
  bentRef.current = bent;
  // AUD-P1-44: the pose of the two leaves, from the hinge angle (0 closed … 180 flat). Both fold
  // inward (inner screens toward each other), each turning on the shared hinge edge.
  //  * book: shown as an open book standing toward the viewer: each half turns by half the fold,
  //    free edges toward the viewer, hinge at the back, so both inner halves stay in view;
  //  * flip: shown like a laptop seen from slightly above: from flat to 90° the upper half stands
  //    facing the viewer and the lower half comes toward the viewer until it lies flat (Flex
  //    mode at 90°); below 90° the lower half stays the base and the upper half leans toward it,
  //    like a lid closing. Either way both inner halves face the viewer and the angle between
  //    the halves is exactly the hinge angle (a + b = 180 − angle).
  const fold2 = 180 - shown;
  const pose: { kind: "book" | "flip"; a: number; b: number } | null =
    bent === "book" ? { kind: "book", a: fold2 / 2, b: fold2 / 2 } : bent === "flip" ? { kind: "flip", a: Math.max(0, fold2 - 90), b: Math.min(fold2, 90) } : null;
  poseRef.current = pose;
  // AUD-P1-44: Galaxy Z Fold7 and Z Flip7 read the fingerprint in the side key (the power button),
  // not under the display: on a foldable the in-display sensor would sit on the hinge (fold) or
  // where the real phone has none (flip). Their selfie cameras: the fold's inner one in the top
  // middle of the right half, its cover one top centre; the flip's main one top centre, and its
  // cover screen (FlexWindow) has the rear cameras beside it, no selfie hole.
  const sideKey = !!kind;
  const camera: "" | "right-half" | "none" =
    kind === "book" && !fold?.cover && !landscape ? "right-half" : kind === "flip" && fold?.cover ? "none" : "";
  const fpWanted = !!auth && (auth.locked || auth.sensor_active) && (device?.interact ?? !inApp);
  const fpTitle = auth?.enrolled
    ? "Fingerprint sensor: hold to touch it with enrolled finger 1"
    : "Fingerprint sensor. No fingerprint is enrolled yet: set a screen lock (PIN), then Settings > Security & privacy > Device unlock > Fingerprint, and hold here each time it asks";
  const keyButton = (
    <div
      className={["pv-sidekey", fpWanted && "ready", finger && "on"].filter(Boolean).join(" ")}
      role="button"
      aria-label="Side key with fingerprint sensor"
      title={fpWanted ? `Side key: ${fpTitle}` : "Side key with the fingerprint sensor (as on Galaxy Z Fold and Flip): hold it when Android asks for a fingerprint"}
      onPointerDown={onFinger(true)}
      onPointerUp={onFinger(false)}
      onPointerCancel={onFinger(false)}
      onContextMenu={(e) => e.preventDefault()}
    >
      {fpWanted && (
        <span className="pv-sidekey-hint" aria-hidden="true">
          <FingerprintGlyph />
        </span>
      )}
    </div>
  );
  useEffect(() => {
    // the half that turns needs a picture before the next frame arrives (the screen may be still)
    const r = rendererRef.current;
    const f = lastFrame.current;
    if (bent && r && f) {
      r.draw(f.data, f.w, f.h);
      mirrorHalf();
    }
  }, [bent]); // eslint-disable-line react-hooks/exhaustive-deps

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
            if (id === "__prepare") {
              e.target.value = profile;
              prepareOthers();
              return;
            }
            const cold = profiles.find((p) => p.profile.id === id)?.warm === false;
            setProfile(id);
            setSwitching({ to: id, t0: Date.now(), cold });
            act(`Switching to ${id}`, () => call("session.start", { profile: id })).finally(() => setSwitching(null));
          }}
        >
          {!profile && <option value="">device…</option>}
          {profiles.map((p) => (
            <option key={p.profile.id} value={p.profile.id}>
              {p.profile.id}
              {p.preparing ? " (getting ready…)" : p.warm === false && p.profile.id !== profile ? " (first start about 2 min)" : ""}
            </option>
          ))}
          {profiles.some((p) => p.warm === false && !p.preparing && p.profile.id !== profile) && (
            <option value="__prepare">Get the other devices ready in the background…</option>
          )}
        </select>
        {/* UI-005: phone keys | device | app | tools, each with a recognisable icon */}
        <span className="pv-group" role="group" aria-label="Phone keys">
          <IconButton label="Back" onClick={() => send({ t: "key", key: "GoBack" })}><ArrowLeft /></IconButton>
          <IconButton label="Home" onClick={() => send({ t: "key", key: "GoHome" })}><House /></IconButton>
          <IconButton label="Recent apps" onClick={() => send({ t: "key", key: "AppSwitch" })}><Layers /></IconButton>
        </span>
        <span className="pv-group" role="group" aria-label="Device">
          <IconButton label="Rotate the phone" disabled={busy} onClick={() => act("Rotate", () => call("device.rotate", {}))}><RotateCwSquare /></IconButton>
        {foldable && <HingeMenu fold={fold} disabled={busy} onAngle={setAngle} />}
          <IconButton label="Screenshot" disabled={busy} onClick={() => act("Screenshot", async () => {
            const r = await call<{ path: string }>("device.screenshot", {});
            return `Saved ${r.path}`;
          })}><Camera /></IconButton>
        </span>
        <span className="pv-group" role="group" aria-label="App">
          <IconButton label="Reload the app (apply saved changes)" disabled={busy} onClick={() => act("Reload", () => call("project.reload", { kind: "hot" }))}><RefreshCw /></IconButton>
          {/* UI-006: stop the running app and its build tools; the emulator stays on */}
          <IconButton
            className="pv-stop"
            label="Stop the app (the emulator stays on)"
            disabled={busy}
            onClick={() =>
              act("Stop", async () => {
                const r = await call<{ stopped: { package?: string } | null }>("project.stop", {});
                return r.stopped ? `Stopped ${r.stopped.package ?? "the app"}. Run it again with umr run.` : "Nothing was running.";
              })
            }
          >
            <Square />
          </IconButton>
        </span>
        {inTauri && <ProjectMenu disabled={busy} onRun={(path, name) => act(`Running ${name}`, async () => {
          const r = await call<{ status: string; package?: string; error?: string }>("project.run", { path, watch: true });
          return r.status === "running" ? `Running ${r.package ?? name}` : `${name}: ${r.error ?? r.status}`;
        })} />}
        <span className="pv-group" role="group" aria-label="Tools">
          <IconButton label="Latency self-test" disabled={busy} onClick={() => act("Self-test", async () => {
            const r = await call<any>("preview.selftest", { taps: 20 });
            const v = r.verdict;
            return `touch→visible p50 ${v.nfr_02b.p50_ms} / p95 ${v.nfr_02b.p95_ms} ms · ${v.nfr_03.fps} fps · ours ${v.nfr_02a.pipeline_p95_ms} ms${v.host_overloaded ? " · host overloaded" : ""}`;
          })}><Gauge /></IconButton>
          <IconButton
            className={errors ? "pv-problems has" : "pv-problems"}
            label={errors ? `${errors} problem${errors === 1 ? "" : "s"} — open problems and logs` : "Problems and logs of the current run"}
            pressed={panel}
            onClick={() => setPanel(!panel)}
          >
            {errors ? <TriangleAlert /> : <ScrollText />}
            {errors > 0 && <span className="pv-count">{errors}</span>}
          </IconButton>
        </span>
        <span className="pv-fps" title="Frames drawn per second while the screen changes; the emulator sends nothing while it is still">
          {device ? (fps === null ? "still" : `${fps} fps`) : ""}
        </span>
      </div>
      <Approvals />
      <RunBanner
        problems={problems}
        onOpen={(fp) => {
          setFocus(fp);
          setPanel(true);
        }}
      />
      <div className="pv-main">
      <div className="pv-stage" ref={stageRef}>
        <div
          className={["pv-phone", landscape && "land", bent && `bent ${bent}`, fold?.cover && "cover", kind && `foldable ${kind}`].filter(Boolean).join(" ")}
          style={phoneStyle(stage, device, landscape)}
          onPointerDown={onPointer("down")}
          onPointerMove={onPointer("move")}
          onPointerUp={onPointer("up")}
          onPointerCancel={onPointer("up")}
          onWheel={onWheel}
          title={fold?.cover ? `${fold.label}: ${fold.cover.label}` : undefined}
        >
          <canvas
            ref={canvasRef}
            className="pv-canvas"
            tabIndex={0}
            onKeyDown={onKey}
            onContextMenu={(e) => e.preventDefault()}
          />
          {pose && (
            <>
              <div
                className={`pv-leaf ${pose.kind} a`}
                style={{ transform: pose.kind === "book" ? `rotateY(${pose.a}deg)` : `rotateX(${-pose.a}deg)` }}
                aria-hidden="true"
              >
                <canvas ref={halfARef} className="pv-half" />
                <div className="pv-back" />
                {pose.kind === "flip" && <div className="pv-camera" aria-hidden="true" />}
                {pose.kind === "flip" && sideKey && keyButton}
              </div>
              <div
                className={`pv-leaf ${pose.kind} b`}
                style={{ transform: pose.kind === "book" ? `rotateY(${-pose.b}deg)` : `rotateX(${pose.b}deg)` }}
                aria-hidden="true"
              >
                <canvas ref={halfBRef} className="pv-half" />
                <div className="pv-back" />
                {pose.kind === "book" && <div className="pv-camera" aria-hidden="true" />}
                {pose.kind === "book" && sideKey && keyButton}
              </div>
            </>
          )}
          {/* the selfie camera of the screen in front (foldables: where the real phone has it) */}
          {!bent && camera !== "none" && <div className={`pv-camera ${camera}`} aria-hidden="true" />}
          {!sideKey && fpWanted && (
            <div
              className={finger ? "pv-fp on" : "pv-fp"}
              role="button"
              aria-label="Fingerprint sensor"
              title={fpTitle}
              onPointerDown={onFinger(true)}
              onPointerUp={onFinger(false)}
              onPointerCancel={onFinger(false)}
              onContextMenu={(e) => e.preventDefault()}
            >
              <FingerprintGlyph />
            </div>
          )}
          {sideKey && !bent && keyButton}
          {(status || switchStage) && <div className="pv-overlay">{switchStage || status}</div>}
        </div>
      </div>
      {panel && (
        <DiagnosticsPanel
          problems={problems}
          focus={focus}
          onClose={() => {
            setPanel(false);
            setFocus(undefined);
          }}
        />
      )}
      </div>
      {note && <div className="pv-note" onClick={() => setNote("")}>{note}</div>}
    </div>
  );
}

/**
 * Phone body size for the stage: the screen keeps the device's aspect (so the frame fills the
 * canvas), a thin bezel around it, corner radii that follow the screen size. CSS variables drive
 * the camera hole and the sensor so they scale with the phone.
 */
function phoneStyle(stage: { w: number; h: number }, device: Hello | null, landscape: boolean): React.CSSProperties {
  const short = device ? Math.min(device.device_width, device.device_height) : 1080;
  const long = device ? Math.max(device.device_width, device.device_height) : 2400;
  const [aw, ah] = landscape ? [long, short] : [short, long];
  const margin = 14;
  // AUD-P3-36: the *screen* has the device's shape and the bezel goes around it. Giving the whole
  // frame the device's shape made the screen inside 2 % taller than the device (309 x 701 for a
  // 1080 x 2400 phone), and the picture sat in it with black bars.
  const room = { w: Math.max(40, stage.w - 2 * margin), h: Math.max(40, stage.h - 2 * margin) };
  const b = Math.max(4, Math.min(9, Math.round(Math.min(room.w, room.h) * 0.02)));
  let sw = Math.max(20, room.w - 2 * b);
  let sh = (sw * ah) / aw;
  if (sh > room.h - 2 * b) {
    sh = Math.max(20, room.h - 2 * b);
    sw = (sh * aw) / ah;
  }
  sw = Math.round(sw);
  sh = Math.round(sh);
  const screenShort = Math.min(sw, sh);
  const radius = Math.round(screenShort * 0.085);
  return {
    width: sw + 2 * b,
    height: sh + 2 * b,
    padding: b,
    borderRadius: radius + b,
    ["--screen-r" as string]: `${radius}px`,
    ["--bz" as string]: `${b}px`,
    ["--outer-r" as string]: `${radius + b}px`,
    ["--short" as string]: `${screenShort}px`,
  };
}

/** The in-display sensor's mark: ridges drawn as nested arcs (own artwork). */
function FingerprintGlyph() {
  const ridges = [
    "M12 4.2c-3.9 0-7 2.6-7.6 6.2",
    "M19.6 10.4C19 6.8 15.9 4.2 12 4.2",
    "M7.4 18.6c-.9-1.5-1.4-3.2-1.4-5 0-3.3 2.7-6 6-6s6 2.7 6 6v.6",
    "M10 20.4c-1.3-1.9-2-4.1-2-6.8 0-2.2 1.8-4 4-4s4 1.8 4 4c0 1-.8 1.8-1.8 1.8",
    "M12.9 20.8c-1.8-1.9-2.9-4.4-2.9-7.2 0-1.1.9-2 2-2s2 .9 2 2c0 2.4 1.5 4.4 3.6 5.2",
    "M15.8 19.8c-1.9-1.4-3.8-3.4-3.8-6.2",
    "M18 16.4c.4-1 .4-2 .4-2.8",
  ];
  return (
    <svg viewBox="0 0 24 24" className="pv-fp-glyph" aria-hidden="true">
      {ridges.map((d) => (
        <path key={d} d={d} fill="none" stroke="currentColor" strokeWidth="1.15" strokeLinecap="round" />
      ))}
    </svg>
  );
}

/** A toolbar button: an icon with its name as tooltip and accessible label. */
/** The hinge of a foldable, as `device.posture` reports it. */
type FoldInfo = {
  kind: "book" | "flip";
  angle: number;
  posture: string;
  label: string;
  steps: number[];
  cover: { width: number; height: number; label: string } | null;
};

/** What Android calls each step (its ranges: up to 30° closed, up to 150° half open). */
const STEP_NAMES: Record<number, string> = { 0: "closed", 30: "closed", 60: "half open", 90: "half open", 120: "half open", 180: "open flat" };

/** AUD-P1-40/41: the hinge control: the posture in words, angle steps and a slider. */
function HingeMenu({ fold, disabled, onAngle }: { fold: FoldInfo | null; disabled: boolean; onAngle: (a: number) => void }) {
  const [open, setOpen] = useState(false);
  const [drag, setDrag] = useState<number | null>(null);
  const angle = fold?.angle ?? 180;
  const steps = fold?.steps?.length ? fold.steps : [0, 30, 60, 90, 120, 180];
  return (
    <span className="pv-projects pv-hinge-menu">
      <button
        className={open ? "pv-hinge-btn on" : "pv-hinge-btn"}
        disabled={disabled || !fold}
        title="Fold or unfold: set the hinge angle"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {fold ? fold.label : "Hinge…"}
      </button>
      {open && fold && (
        <div className="pv-menu pv-hinge-pop" role="menu" onMouseLeave={() => setOpen(false)}>
          <div className="pv-hinge-steps">
            {steps.map((a) => (
              <button
                key={a}
                role="menuitemradio"
                aria-checked={Math.round(angle) === a}
                className={Math.round(angle) === a ? "on" : ""}
                onClick={() => {
                  setOpen(false);
                  onAngle(a);
                }}
              >
                <strong>{a}°</strong>
                <span>{STEP_NAMES[a] ?? (a <= 30 ? "closed" : a <= 150 ? "half open" : "open")}</span>
              </button>
            ))}
          </div>
          <label className="pv-hinge-slider">
            <span>Any angle: {drag ?? Math.round(angle)}°</span>
            <input
              type="range"
              min={0}
              max={180}
              step={1}
              value={drag ?? Math.round(angle)}
              onChange={(e) => setDrag(Number(e.target.value))}
              onPointerUp={() => {
                if (drag !== null) onAngle(drag);
                setDrag(null);
              }}
              onKeyUp={() => {
                if (drag !== null) onAngle(drag);
                setDrag(null);
              }}
            />
          </label>
          {fold.cover && <p className="pv-menu-empty">On the {fold.cover.label} ({fold.cover.width} × {fold.cover.height})</p>}
        </div>
      )}
    </span>
  );
}

/** AUD-P3-37: switch the running project from the phone window (recent projects, one click). */
function ProjectMenu({ disabled, onRun }: { disabled: boolean; onRun: (path: string, name: string) => void }) {
  const [open, setOpen] = useState(false);
  const [list, setList] = useState<{ path: string; adapter: string; exists: boolean }[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const name = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
  const load = () =>
    call<{ recent: { path: string; adapter: string; exists: boolean }[]; run: { project: string } | null }>("home.summary")
      .then((h) => {
        setList(h.recent.slice(0, 8));
        setCurrent(h.run?.project ?? null);
      })
      .catch(() => setList([]));
  return (
    <span className="pv-projects">
      <IconButton label="Switch project: run another recent project on this phone" disabled={disabled} pressed={open} onClick={() => { if (!open) load(); setOpen(!open); }}>
        <FolderOpen />
      </IconButton>
      {open && (
        <div className="pv-menu" role="menu" onMouseLeave={() => setOpen(false)}>
          {list.length === 0 && <div className="pv-menu-empty">No recent projects yet.</div>}
          {list.map((r) => (
            <button
              key={r.path}
              role="menuitem"
              className={r.path === current ? "on" : ""}
              disabled={!r.exists || r.path === current}
              title={r.exists ? r.path : `${r.path} (folder not found)`}
              onClick={() => {
                setOpen(false);
                onRun(r.path, name(r.path));
              }}
            >
              <strong>{name(r.path)}</strong> <span>{r.path === current ? "running" : FRAMEWORK_SHORT[r.adapter] ?? r.adapter}</span>
            </button>
          ))}
        </div>
      )}
    </span>
  );
}
const FRAMEWORK_SHORT: Record<string, string> = { "gradle-android": "Kotlin / Java", flutter: "Flutter", "react-native": "React Native" };

function IconButton({
  label,
  onClick,
  disabled,
  className,
  pressed,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  className?: string;
  pressed?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className={className ? `pv-icon ${className}` : "pv-icon"}
      title={label}
      aria-label={label}
      aria-pressed={pressed}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
