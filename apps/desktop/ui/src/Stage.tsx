/**
 * UI-007: the playful right-hand panel of the setup window. A little phone character ("Pocket")
 * hangs from a rail by a rope: drag it and fling it, click anywhere in the panel and the rail
 * trolley carries it there, click it and it reacts. Developer chips float behind it: drag and
 * throw them (they bounce off Pocket and spring home), click them for their own reaction.
 * Purely decorative (aria-hidden); it says one short line when the wizard step changes.
 *
 * AUD-P2-15 motion model, stepped at a fixed 120 Hz and drawn interpolated:
 *  - the rope is a stiff damped spring (a little give, no jolt at full length);
 *  - the body has its own angle and spin: it follows the rope with lag and overshoot and swings
 *    against the hook's acceleration (a two-part pendulum), with a slight squash at speed;
 *  - the trolley, the drag and the chips are springs, never jumps; walls push back softly;
 *  - a breeze is a gust spread over a second; fling speed is averaged over the last 100 ms.
 * AUD-P1-12: every animation pauses when the window is hidden, loses focus or nothing happens for
 * 30 s, and the simulation stops when everything is at rest.
 * To remove the panel: drop <Stage> from App.tsx and the "Stage" block of styles.css.
 */
import { useEffect, useRef } from "react";
import gsap from "gsap";
import { Braces, Bug, Coffee, Cog, GitBranch, Play, Smartphone, Terminal, type LucideIcon } from "lucide-react";

const STEP_LINE: Record<string, string> = {
  scan: "Let me peek at your toolbox…",
  project: "Which app are we running today?",
  plan: "Here's what we still need.",
  licences: "The fine print, then the fun part.",
  install: "Fetching the good stuff. Coffee?",
  ready: "Everything checks out. Let's run it!",
  run: "Booting a phone just for you.",
  agents: "Bring your AI coding buddies.",
  home: "Welcome back! Pick up where you left off.",
};

const POKES = ["Wheee!", "Hot reload me again!", "I run on coffee and Gradle.", "Careful, I'm in debug mode.", "No crashes on my watch.", "Again! Again!", "Build successful. Probably."];
const OUCH = ["Hey!", "Oof — nice throw.", "Ow, that's a merge conflict.", "Missed the bug, hit me."];

type Kind = "braces" | "bug" | "coffee" | "cog" | "git" | "terminal" | "play" | "phone";
const CHIPS: { kind: Kind; Icon: LucideIcon; c: string; x: number; y: number; s: number; d: number; say: string }[] = [
  { kind: "braces", Icon: Braces, c: "var(--lime)", x: 10, y: 30, s: 50, d: 1.2, say: "Curly braces: my favourite hugs." },
  { kind: "bug", Icon: Bug, c: "var(--c-coral)", x: 76, y: 20, s: 46, d: 0.8, say: "A bug! Catch it if you can." },
  { kind: "coffee", Icon: Coffee, c: "var(--c-sun)", x: 18, y: 62, s: 44, d: 1.0, say: "Refill! Thank you." },
  { kind: "cog", Icon: Cog, c: "var(--c-sky)", x: 80, y: 50, s: 52, d: 1.4, say: "Gradle is thinking. Give it a second." },
  { kind: "git", Icon: GitBranch, c: "var(--c-pink)", x: 8, y: 86, s: 40, d: 0.6, say: "New branch. Commit early, commit often." },
  { kind: "terminal", Icon: Terminal, c: "var(--lime)", x: 58, y: 80, s: 42, d: 0.9, say: "umr run — that's the whole trick." },
  { kind: "play", Icon: Play, c: "var(--c-sky)", x: 84, y: 82, s: 38, d: 0.7, say: "Press play — dance break!" },
  { kind: "phone", Icon: Smartphone, c: "var(--c-sun)", x: 40, y: 92, s: 36, d: 0.5, say: "That's my cousin." },
];

const RAIL_Y = 26;
const SCALE = 1.25;
const DT = 1 / 120; // simulation step
const G = 1900; // px/s²
const IDLE_MS = 30_000;

type V = { x: number; y: number };
type Sample = { t: number; x: number; y: number };

/** Speed over the last 100 ms of pointer samples (px/s), not just the last two moves. */
function releaseVelocity(s: Sample[]): V {
  if (s.length < 2) return { x: 0, y: 0 };
  const last = s[s.length - 1];
  const first = s.find((q) => last.t - q.t <= 100) ?? s[0];
  const dt = Math.max(16, last.t - first.t) / 1000;
  const lim = (n: number) => Math.max(-2600, Math.min(2600, n));
  return { x: lim((last.x - first.x) / dt), y: lim((last.y - first.y) / dt) };
}

export function Stage({ step }: { step: string }) {
  const root = useRef<HTMLElement>(null);
  const rope = useRef<SVGPathElement>(null);
  const trolley = useRef<SVGGElement>(null);
  const fig = useRef<SVGGElement>(null);
  const body = useRef<SVGGElement>(null);
  const eyes = useRef<SVGGElement>(null);
  const pupils = useRef<SVGGElement>(null);
  const arm = useRef<SVGGElement>(null);
  const mug = useRef<SVGGElement>(null);
  const steam = useRef<SVGGElement>(null);
  const branch = useRef<SVGPathElement>(null);
  const bubblePos = useRef<HTMLDivElement>(null);
  const bubble = useRef<HTMLDivElement>(null);
  const api = useRef<{ say(text: string, delay?: number): void; wave(): void } | null>(null);
  const first = useRef(true);

  useEffect(() => {
    const el = root.current!;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const ctx = gsap.context(() => undefined, el);
    const add = (f: () => void) => ctx.add(f);
    const clamp = gsap.utils.clamp;

    let W = el.clientWidth || 400;
    let H = el.clientHeight || 800;
    let L = Math.max(150, H * 0.34);

    // ---- state ------------------------------------------------------------------------------
    const tr = { x: W * 0.5, v: 0, target: W * 0.5, px: W * 0.5 }; // trolley on the rail
    const p = reduce ? { x: tr.x, y: RAIL_Y + L } : { x: tr.x + 110, y: RAIL_Y + 20 }; // the hook
    const v = { x: 0, y: 0 };
    const prev = { x: p.x, y: p.y, th: 0 };
    let th = 0; // body angle (rad, + = clockwise on screen)
    let om = 0; // its spin
    let ax = 0; // the hook's sideways acceleration, felt by the body
    let gust: { f: number; t: number; dur: number } | null = null;
    let pointer: V | null = null;
    let look: { x: number; y: number; until: number } | null = null;

    type Chip = { el: HTMLElement; pos: HTMLElement; kind: Kind; r: number; hx: number; hy: number; ox: number; oy: number; vx: number; vy: number; pox: number; poy: number };
    const chips: Chip[] = Array.from(el.querySelectorAll<HTMLElement>(".chip-phys")).map((n, i) => ({
      el: n,
      pos: n.parentElement as HTMLElement,
      kind: CHIPS[i].kind,
      r: CHIPS[i].s / 2,
      hx: 0,
      hy: 0,
      ox: 0,
      oy: 0,
      vx: 0,
      vy: 0,
      pox: 0,
      poy: 0,
    }));
    const homes = () => chips.forEach((c, i) => ((c.hx = (CHIPS[i].x / 100) * W), (c.hy = (CHIPS[i].y / 100) * H)));
    homes();

    type Drag = { kind: "fig" | "chip"; i: number; ox: number; oy: number; sx: number; sy: number; moved: boolean; samples: Sample[] };
    let drag: Drag | null = null;
    let target: V = { x: p.x, y: p.y }; // where the dragged thing is pulled to
    let down: V | null = null;
    let bw = 0;
    let bh = 0;
    let raf = 0;
    let last = 0;
    let acc = 0;
    let still = 0;
    let bumpCooldown = 0;

    // ---- simulation -------------------------------------------------------------------------
    function bodyCenter(): V {
      // the body's middle hangs 66 units below the hook, turned with the body
      const d = 66 * SCALE;
      return { x: p.x - Math.sin(th) * d, y: p.y + Math.cos(th) * d };
    }

    function stepSim(dt: number) {
      prev.x = p.x;
      prev.y = p.y;
      prev.th = th;
      tr.px = tr.x;
      chips.forEach((c) => ((c.pox = c.ox), (c.poy = c.oy)));

      // trolley: a spring to its target, soft at the rail's ends
      const kT = 70;
      tr.v += (kT * (tr.target - tr.x) - 2 * Math.sqrt(kT) * 0.9 * tr.v) * dt;
      tr.x += tr.v * dt;
      if (tr.x < 46) tr.v += (46 - tr.x) * 400 * dt;
      if (tr.x > W - 46) tr.v -= (tr.x - (W - 46)) * 400 * dt;

      // the hook
      const vx0 = v.x;
      let fx = 0;
      let fy = 0;
      if (drag?.kind === "fig") {
        const k = 900;
        fx = k * (target.x - p.x) - 2 * Math.sqrt(k) * v.x;
        fy = k * (target.y - p.y) - 2 * Math.sqrt(k) * v.y;
      } else {
        fy = G;
        if (gust) {
          gust.t += dt;
          fx += gust.f * Math.sin((Math.PI * gust.t) / gust.dur);
          if (gust.t >= gust.dur) gust = null;
        }
        const damp = Math.exp(-1.15 * dt);
        v.x *= damp;
        v.y *= damp;
      }
      // the rope: a stiff damped spring once taut
      const dx = p.x - tr.x;
      const dy = p.y - RAIL_Y;
      const d = Math.hypot(dx, dy) || 1;
      if (d > L) {
        const nx = dx / d;
        const ny = dy / d;
        const vr = (v.x - tr.v) * nx + v.y * ny;
        const f = -1500 * (d - L) - 45 * vr;
        fx += f * nx;
        fy += f * ny;
        // a hard swing tugs the trolley a little
        if (!drag) tr.v -= f * nx * 0.00006;
      }
      // soft walls and floor
      const wall = (over: number, vel: number) => over * 700 + (vel > 0 ? vel * 18 : 0);
      if (p.x < 46) fx += wall(46 - p.x, -v.x);
      if (p.x > W - 46) fx -= wall(p.x - (W - 46), v.x);
      if (p.y > H - 190) fy -= wall(p.y - (H - 190), v.y);
      v.x += fx * dt;
      v.y += fy * dt;
      p.x += v.x * dt;
      p.y += v.y * dt;
      // never stretch the rope far, whatever the pull
      const d2 = Math.hypot(p.x - tr.x, p.y - RAIL_Y);
      if (d2 > L * 1.2) {
        p.x = tr.x + ((p.x - tr.x) / d2) * L * 1.2;
        p.y = RAIL_Y + ((p.y - RAIL_Y) / d2) * L * 1.2;
      }
      ax = ax * 0.8 + ((v.x - vx0) / dt) * 0.2;

      // the body: follows the rope's angle with lag and overshoot, swings against acceleration
      const dd = Math.hypot(p.x - tr.x, p.y - RAIL_Y);
      const slack = Math.max(0, L - dd);
      let phi = dd > 1 ? -Math.atan2(p.x - tr.x, p.y - RAIL_Y) : 0;
      phi *= Math.max(0, 1 - slack / 60);
      const watch = look && performance.now() < look.until ? look : pointer;
      if (watch) {
        const lean = clamp(-1, 1, (watch.x - p.x) / 300);
        if (Math.abs(watch.y - p.y) < 320) phi -= lean * 0.07;
      }
      const kB = 55;
      om += (-kB * (th - phi) - 4.2 * om + ax * 0.00035) * dt;
      th += om * dt;

      // chips: springs home; dragged ones follow the pointer; they bounce off the body
      const bc = bodyCenter();
      chips.forEach((c, i) => {
        let cfx: number;
        let cfy: number;
        if (drag?.kind === "chip" && drag.i === i) {
          const k = 700;
          cfx = k * (target.x - (c.hx + c.ox)) - 2 * Math.sqrt(k) * c.vx;
          cfy = k * (target.y - (c.hy + c.oy)) - 2 * Math.sqrt(k) * c.vy;
        } else {
          // soft and lightly damped: a throw carries across the panel, then it bobs home in ~2 s
          const k = 14;
          cfx = -k * c.ox - 2.6 * c.vx;
          cfy = -k * c.oy - 2.6 * c.vy;
          // the bug runs from the pointer
          if (c.kind === "bug" && pointer) {
            const ex = c.hx + c.ox - pointer.x;
            const ey = c.hy + c.oy - pointer.y;
            const e = Math.hypot(ex, ey) || 1;
            if (e < 120) {
              cfx += (ex / e) * (120 - e) * 60;
              cfy += (ey / e) * (120 - e) * 60;
            }
          }
        }
        c.vx += cfx * dt;
        c.vy += cfy * dt;
        c.ox += c.vx * dt;
        c.oy += c.vy * dt;
        // the panel's edges: a thrown chip bounces back instead of leaving the view
        const left = c.r + 4 - c.hx;
        const right = W - c.r - 4 - c.hx;
        const top = RAIL_Y + c.r + 8 - c.hy;
        const bottom = H - c.r - 4 - c.hy;
        if (c.ox < left || c.ox > right) {
          c.ox = clamp(left, right, c.ox);
          c.vx *= -0.55;
        }
        if (c.oy < top || c.oy > bottom) {
          c.oy = clamp(top, bottom, c.oy);
          c.vy *= -0.55;
        }
        // collision with Pocket's body
        const cx = c.hx + c.ox;
        const cy = c.hy + c.oy;
        const nx0 = cx - bc.x;
        const ny0 = cy - bc.y;
        const dist = Math.hypot(nx0, ny0) || 1;
        const min = c.r + 46;
        if (dist < min) {
          const nx = nx0 / dist;
          const ny = ny0 / dist;
          c.ox += nx * (min - dist);
          c.oy += ny * (min - dist);
          const rel = (c.vx - v.x) * nx + (c.vy - v.y) * ny;
          if (rel < 0) {
            const j = (-(1 + 0.55) * rel) / (1 + 1 / 3); // chip mass 1, Pocket 3
            c.vx += j * nx;
            c.vy += j * ny;
            if (!(drag?.kind === "fig")) {
              v.x -= (j * nx) / 3;
              v.y -= (j * ny) / 3;
              om += -nx * j * 0.0012;
            }
            if (-rel > 700 && performance.now() > bumpCooldown) {
              bumpCooldown = performance.now() + 3000;
              say(OUCH[Math.floor(Math.random() * OUCH.length)]);
            }
          }
        }
      });
    }

    function resting() {
      const chipsMoving = chips.some((c) => Math.abs(c.vx) + Math.abs(c.vy) > 2 || Math.abs(c.ox) + Math.abs(c.oy) > 0.5);
      return (
        !drag &&
        !gust &&
        Math.hypot(v.x, v.y) < 2 &&
        Math.abs(tr.v) < 1 &&
        Math.abs(tr.target - tr.x) < 0.5 &&
        Math.abs(om) < 0.01 &&
        !chipsMoving
      );
    }

    // ---- drawing (interpolated between the last two steps) ------------------------------------
    function render(alpha: number) {
      const ix = prev.x + (p.x - prev.x) * alpha;
      const iy = prev.y + (p.y - prev.y) * alpha;
      const ith = prev.th + (th - prev.th) * alpha;
      const itx = tr.px + (tr.x - tr.px) * alpha;
      const dx = ix - itx;
      const dy = iy - RAIL_Y;
      const d = Math.hypot(dx, dy);
      // one curve for slack and taut rope: the sag fades smoothly to a straight line
      const sag = Math.max(0, L - d) * 0.7;
      rope.current?.setAttribute("d", `M${itx.toFixed(1)} ${RAIL_Y} Q${((itx + ix) / 2).toFixed(1)} ${((RAIL_Y + iy) / 2 + sag).toFixed(1)} ${ix.toFixed(1)} ${iy.toFixed(1)}`);
      trolley.current?.setAttribute("transform", `translate(${itx.toFixed(1)} ${RAIL_Y}) rotate(${clamp(-8, 8, -tr.v * 0.02).toFixed(2)})`);
      // a slight stretch at speed, along the body
      const speed = Math.hypot(v.x, v.y);
      const sy = 1 + Math.min(0.06, speed / 9000);
      const deg = (ith * 180) / Math.PI;
      fig.current?.setAttribute("transform", `translate(${ix.toFixed(1)} ${iy.toFixed(1)}) rotate(${deg.toFixed(2)}) scale(${(SCALE / Math.sqrt(sy)).toFixed(4)} ${(SCALE * sy).toFixed(4)})`);
      if (bubblePos.current) {
        // AUD-P2-31: beside Pocket, but always fully inside the panel (never over the hint or an edge)
        const right = ix + 84 + bw < W - 12;
        const x = clamp(12, Math.max(12, W - 12 - bw), right ? ix + 84 : ix - 84 - bw);
        const y = clamp(12, Math.max(12, H - 40 - bh), iy + 28);
        bubblePos.current.style.transform = `translate(${x.toFixed(1)}px, ${y.toFixed(1)}px)`;
      }
      chips.forEach((c) => {
        const ox = c.pox + (c.ox - c.pox) * alpha;
        const oy = c.poy + (c.oy - c.poy) * alpha;
        c.el.style.transform = `translate(${ox.toFixed(1)}px, ${oy.toFixed(1)}px)`;
      });
      eyesTo(look && performance.now() < look.until ? look : pointer, ix, iy);
    }

    function eyesTo(t: V | null, ix: number, iy: number) {
      if (!t) return;
      const dx = t.x - ix;
      const dy = t.y - (iy + 52 * SCALE);
      const d = Math.hypot(dx, dy) || 1;
      const k = Math.min(2.6, d / 60) / d;
      pupils.current?.setAttribute("transform", `translate(${(dx * k).toFixed(2)} ${(dy * k).toFixed(2)})`);
    }

    function frame(t: number) {
      acc += Math.min(0.05, Math.max(0, (t - last) / 1000));
      last = t;
      let n = 0;
      while (acc >= DT && n < 12) {
        stepSim(DT);
        acc -= DT;
        n++;
      }
      render(acc / DT);
      still = resting() ? still + 1 : 0;
      if (still > 45 || paused) {
        raf = 0;
        return;
      }
      raf = requestAnimationFrame(frame);
    }
    function wake() {
      still = 0;
      if (!raf && !paused) {
        last = performance.now();
        acc = 0;
        raf = requestAnimationFrame(frame);
      }
    }

    // ---- AUD-P1-12: pause when nobody is looking ---------------------------------------------
    let paused = false;
    let lastActivity = performance.now();
    const ticker = gsap.ticker as unknown as { sleep?: () => void; wake?: () => void };
    function pause() {
      if (paused) return;
      paused = true;
      gsap.globalTimeline.pause();
      ticker.sleep?.();
      cancelAnimationFrame(raf);
      raf = 0;
    }
    function resume() {
      lastActivity = performance.now();
      // moving the pointer over the window counts as looking at it, focused or not
      if (!paused || document.hidden) return;
      paused = false;
      ticker.wake?.();
      gsap.globalTimeline.resume();
      wake();
    }
    const activity = () => {
      lastActivity = performance.now();
      if (paused) resume();
    };
    const onVisibility = () => (document.hidden ? pause() : resume());
    const idleTimer = window.setInterval(() => {
      if (!paused && performance.now() - lastActivity > IDLE_MS) pause();
    }, 5000);

    // ---- actions ----------------------------------------------------------------------------
    function moveTo(x: number) {
      tr.target = clamp(46, W - 46, x);
      wake();
    }

    let hide: gsap.core.Tween | null = null;
    function say(text: string, delay = 0) {
      if (!text) return;
      activity();
      add(() => {
        gsap.delayedCall(delay, () => {
          const b = bubble.current;
          if (!b) return;
          b.textContent = text;
          bw = b.offsetWidth;
          bh = b.offsetHeight;
          render(1);
          hide?.kill();
          add(() => {
            gsap.fromTo(b, { autoAlpha: 0, scale: 0.8, y: 6 }, { autoAlpha: 1, scale: 1, y: 0, duration: 0.35, ease: "back.out(2.2)", transformOrigin: "0% 0%" });
            hide = gsap.delayedCall(4.2, () => {
              gsap.to(b, { autoAlpha: 0, scale: 0.92, duration: 0.3 });
            });
          });
        });
      });
    }

    function wave() {
      if (reduce || !arm.current) return;
      add(() => {
        gsap.fromTo(arm.current, { rotation: 0 }, { rotation: -42, transformOrigin: "100% 0%", duration: 0.16, yoyo: true, repeat: 5, ease: "sine.inOut" });
      });
    }

    function squash(amount = 0.16) {
      if (reduce || !body.current) return;
      add(() => {
        gsap.fromTo(body.current, { scaleX: 1 + amount, scaleY: 1 - amount }, { scaleX: 1, scaleY: 1, duration: 0.7, ease: "elastic.out(1, 0.35)", transformOrigin: "50% 0%" });
      });
    }

    function poke() {
      say(POKES[Math.floor(Math.random() * POKES.length)]);
      wave();
      squash();
      v.x += (Math.random() < 0.5 ? -1 : 1) * gsap.utils.random(350, 600);
      v.y -= 250;
      om += gsap.utils.random(-3, 3);
      wake();
    }

    /** Sparks and a ripple ring around a chip, in its colour. */
    function burst(c: Chip, colour: string) {
      if (reduce) return;
      const host = c.pos;
      const ring = document.createElement("div");
      ring.className = "chip-ring";
      ring.style.borderColor = colour;
      host.appendChild(ring);
      add(() => {
        gsap.fromTo(ring, { x: c.ox, y: c.oy, scale: 0.6, opacity: 0.7 }, { scale: 2.3, opacity: 0, duration: 0.7, ease: "power2.out", onComplete: () => ring.remove() });
        for (let k = 0; k < 7; k++) {
          const s = document.createElement("div");
          s.className = "chip-spark";
          s.style.background = colour;
          host.appendChild(s);
          const a = (Math.PI * 2 * k) / 7 + Math.random() * 0.5;
          const r = gsap.utils.random(38, 64);
          gsap.fromTo(s, { x: c.ox, y: c.oy, scale: 1, opacity: 1 }, { x: c.ox + Math.cos(a) * r, y: c.oy + Math.sin(a) * r, scale: 0, opacity: 0, duration: gsap.utils.random(0.5, 0.8), ease: "power3.out", onComplete: () => s.remove() });
        }
      });
    }

    function chipClick(i: number) {
      const c = chips[i];
      const def = CHIPS[i];
      const chipEl = c.el.querySelector<HTMLElement>(".chip");
      const centre = { x: c.hx + c.ox, y: c.hy + c.oy };
      look = { ...centre, until: performance.now() + 1600 };
      say(def.say);
      burst(c, def.c);
      if (!reduce && chipEl) {
        add(() => {
          gsap
            .timeline()
            .to(chipEl, { scale: 1.28, duration: 0.12, ease: "power2.out" })
            .to(chipEl, { scale: 1, duration: 0.8, ease: "elastic.out(1.1, 0.35)" })
            .to(chipEl, { rotation: `+=${def.kind === "cog" ? 720 : 360}`, duration: def.kind === "cog" ? 1.3 : 0.7, ease: "power3.out" }, 0);
        });
      }
      switch (def.kind) {
        case "bug": {
          // it scurries off and wiggles
          c.vx += gsap.utils.random(-500, 500);
          c.vy += gsap.utils.random(-500, 500);
          if (!reduce && chipEl) add(() => gsap.fromTo(chipEl, { rotation: -15 }, { rotation: 15, duration: 0.08, repeat: 7, yoyo: true, ease: "sine.inOut" }));
          break;
        }
        case "coffee": {
          // Pocket's mug gets a refill
          if (!reduce && mug.current) {
            add(() => {
              gsap.fromTo(mug.current, { scale: 1.35 }, { scale: 1, duration: 0.9, ease: "elastic.out(1, 0.3)", transformOrigin: "50% 100%" });
              if (steam.current) gsap.fromTo(steam.current.children, { y: 0, opacity: 1 }, { y: -12, opacity: 0, duration: 0.9, stagger: 0.12, repeat: 2, ease: "sine.out" });
            });
          }
          break;
        }
        case "play": {
          // a little dance: three hops left and right, waving
          wave();
          [0, 0.35, 0.7].forEach((t, k) =>
            add(() =>
              gsap.delayedCall(t, () => {
                v.x += (k % 2 ? 1 : -1) * 420;
                v.y -= 260;
                om += (k % 2 ? -1 : 1) * 4;
                squash(0.1);
                wake();
              }),
            ),
          );
          break;
        }
        case "git": {
          // a branch line from this chip to another one
          const other = chips[(i + 1 + Math.floor(Math.random() * (chips.length - 1))) % chips.length];
          const b = branch.current;
          if (b && !reduce) {
            const x1 = c.hx + c.ox;
            const y1 = c.hy + c.oy;
            const x2 = other.hx + other.ox;
            const y2 = other.hy + other.oy;
            b.setAttribute("d", `M${x1} ${y1} C${x1} ${(y1 + y2) / 2} ${x2} ${(y1 + y2) / 2} ${x2} ${y2}`);
            const len = b.getTotalLength();
            add(() => {
              gsap
                .timeline()
                .set(b, { strokeDasharray: len, strokeDashoffset: len, opacity: 1 })
                .to(b, { strokeDashoffset: 0, duration: 0.6, ease: "power2.inOut" })
                .to(b, { opacity: 0, duration: 0.5, delay: 0.8 });
            });
          }
          break;
        }
        default:
          break;
      }
      wake();
    }

    api.current = { say, wave };

    // ---- pointer ------------------------------------------------------------------------------
    const local = (e: PointerEvent): V => {
      const r = el.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const onDown = (e: PointerEvent) => {
      activity();
      const t = e.target as Element;
      const at = local(e);
      const sample = { t: performance.now(), ...at };
      if (t.closest(".fig")) {
        drag = { kind: "fig", i: -1, ox: p.x - at.x, oy: p.y - at.y, sx: at.x, sy: at.y, moved: false, samples: [sample] };
        target = { x: p.x, y: p.y };
        el.classList.add("dragging");
        wake();
        e.preventDefault();
        return;
      }
      const chip = t.closest<HTMLElement>(".chip-phys");
      if (chip) {
        const i = chips.findIndex((c) => c.el === chip);
        const c = chips[i];
        drag = { kind: "chip", i, ox: c.hx + c.ox - at.x, oy: c.hy + c.oy - at.y, sx: at.x, sy: at.y, moved: false, samples: [sample] };
        target = { x: c.hx + c.ox, y: c.hy + c.oy };
        el.classList.add("dragging");
        e.preventDefault();
        return;
      }
      down = at;
    };
    const onMove = (e: PointerEvent) => {
      activity();
      const at = local(e);
      pointer = at;
      if (!raf) eyesTo(pointer, p.x, p.y);
      if (!drag) {
        // the bug notices the pointer
        if (chips.some((c) => c.kind === "bug" && Math.hypot(c.hx + c.ox - at.x, c.hy + c.oy - at.y) < 120)) wake();
        return;
      }
      if (!drag.moved && Math.hypot(at.x - drag.sx, at.y - drag.sy) > 5) drag.moved = true;
      drag.samples.push({ t: performance.now(), ...at });
      if (drag.samples.length > 12) drag.samples.shift();
      target = { x: at.x + drag.ox, y: at.y + drag.oy };
      if (drag.kind === "fig") {
        // pulled past the rope's length: the trolley follows along the rail
        const ddx = target.x - tr.x;
        const ddy = target.y - RAIL_Y;
        if (Math.hypot(ddx, ddy) > L) {
          const reach = Math.sqrt(Math.max(0, L * L - ddy * ddy));
          tr.target = clamp(46, W - 46, target.x - Math.sign(ddx) * reach);
        }
      }
      wake();
    };
    const onUp = () => {
      if (drag) {
        const d = drag;
        drag = null;
        el.classList.remove("dragging");
        const rv = releaseVelocity(d.samples);
        if (d.kind === "fig") {
          if (d.moved) {
            v.x = rv.x;
            v.y = rv.y;
          } else poke();
        } else if (d.moved) {
          chips[d.i].vx = rv.x;
          chips[d.i].vy = rv.y;
        } else chipClick(d.i);
        wake();
        return;
      }
      if (down) {
        const at = pointer ?? down;
        if (Math.hypot(at.x - down.x, at.y - down.y) < 6) moveTo(at.x);
        down = null;
      }
    };
    const onLeave = () => {
      pointer = null;
    };
    el.addEventListener("pointerdown", onDown);
    el.addEventListener("pointerleave", onLeave);
    window.addEventListener("pointermove", onMove, { passive: true });
    window.addEventListener("pointerup", onUp);
    window.addEventListener("keydown", activity);
    window.addEventListener("wheel", activity, { passive: true });
    window.addEventListener("focus", resume);
    window.addEventListener("blur", pause);
    document.addEventListener("visibilitychange", onVisibility);

    const ro = new ResizeObserver(() => {
      W = el.clientWidth;
      H = el.clientHeight;
      L = Math.max(150, H * 0.34);
      tr.target = clamp(46, W - 46, tr.target);
      homes();
      wake();
    });
    ro.observe(el);

    // ---- ambient (paused with everything else) ----------------------------------------------
    const floats = new Map<HTMLElement, gsap.core.Tween>();
    render(1);
    if (!reduce) {
      wake(); // the drop-in entrance: it falls onto its rope and swings
      add(() => {
        el.querySelectorAll<HTMLElement>(".chip-float").forEach((f) => {
          floats.set(f, gsap.to(f, { y: gsap.utils.random(-16, 16), rotation: gsap.utils.random(-12, 12), duration: gsap.utils.random(2.4, 4.4), yoyo: true, repeat: -1, ease: "sine.inOut", delay: gsap.utils.random(0, 1.2) }));
        });
        gsap.from(el.querySelectorAll(".chip"), { scale: 0, rotation: -90, duration: 0.7, ease: "back.out(2)", stagger: 0.06, delay: 0.25 });
        el.querySelectorAll(".blob").forEach((b) => {
          gsap.to(b, { x: gsap.utils.random(-50, 50), y: gsap.utils.random(-40, 40), duration: gsap.utils.random(8, 13), yoyo: true, repeat: -1, ease: "sine.inOut" });
        });
        if (steam.current) {
          gsap.fromTo(steam.current.children, { y: 0, opacity: 0.75 }, { y: -7, opacity: 0, duration: 1.6, repeat: -1, stagger: 0.55, ease: "sine.out" });
        }
        const blink = () => {
          add(() => {
            gsap
              .timeline({ onComplete: () => add(() => gsap.delayedCall(gsap.utils.random(2.5, 6), blink)) })
              .to(eyes.current, { scaleY: 0.1, transformOrigin: "50% 50%", duration: 0.07 })
              .to(eyes.current, { scaleY: 1, duration: 0.09 });
          });
        };
        gsap.delayedCall(2, blink);
        const breeze = () => {
          if (!drag) {
            // a gust spread over about a second, not a kick
            gust = { f: gsap.utils.random(-260, 260), t: 0, dur: gsap.utils.random(0.8, 1.4) };
            wake();
          }
          add(() => gsap.delayedCall(gsap.utils.random(7, 12), breeze));
        };
        gsap.delayedCall(7, breeze);
      });
    }
    // hovering a chip stops its float, so it is easy to hit
    const onOver = (e: PointerEvent) => {
      const f = (e.target as Element).closest<HTMLElement>(".chip-float");
      floats.get(f as HTMLElement)?.pause();
    };
    const onOut = (e: PointerEvent) => {
      const f = (e.target as Element).closest<HTMLElement>(".chip-float");
      if (f && !f.contains(e.relatedTarget as Node)) floats.get(f)?.resume();
    };
    el.addEventListener("pointerover", onOver);
    el.addEventListener("pointerout", onOut);
    // parallax: the chips lean away from the pointer, deeper ones more
    const movers = reduce
      ? []
      : Array.from(el.querySelectorAll<HTMLElement>(".chip-pos")).map((n) => ({
          d: Number(n.dataset.d),
          x: gsap.quickTo(n, "x", { duration: 0.9, ease: "power3" }),
          y: gsap.quickTo(n, "y", { duration: 0.9, ease: "power3" }),
        }));
    const onPar = (e: PointerEvent) => {
      if (paused) return;
      const fx = e.clientX / window.innerWidth - 0.5;
      const fy = e.clientY / window.innerHeight - 0.5;
      movers.forEach((m) => {
        m.x(-fx * 26 * m.d);
        m.y(-fy * 18 * m.d);
      });
    };
    window.addEventListener("pointermove", onPar, { passive: true });
    // a hidden window starts paused; an unfocused one still plays its entrance, then idles out
    if (document.hidden) pause();

    return () => {
      cancelAnimationFrame(raf);
      window.clearInterval(idleTimer);
      ro.disconnect();
      el.removeEventListener("pointerdown", onDown);
      el.removeEventListener("pointerleave", onLeave);
      el.removeEventListener("pointerover", onOver);
      el.removeEventListener("pointerout", onOut);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointermove", onPar);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("keydown", activity);
      window.removeEventListener("wheel", activity);
      window.removeEventListener("focus", resume);
      window.removeEventListener("blur", pause);
      document.removeEventListener("visibilitychange", onVisibility);
      api.current = null;
      if (paused) {
        ticker.wake?.();
        gsap.globalTimeline.resume();
      }
      ctx.revert();
    };
  }, []);

  useEffect(() => {
    const line = STEP_LINE[step];
    const delay = first.current ? 1.1 : 0.15;
    first.current = false;
    api.current?.say(line ?? "", delay);
    api.current?.wave();
  }, [step]);

  return (
    <aside className="stage" ref={root} aria-hidden="true">
      <div className="blob b1" />
      <div className="blob b2" />
      <div className="blob b3" />
      {CHIPS.map((c, i) => (
        <div key={i} className="chip-pos" data-d={c.d} style={{ left: `${c.x}%`, top: `${c.y}%` }}>
          <div className="chip-phys">
            <div className="chip-float">
              <div className="chip" data-i={i} style={{ width: c.s, height: c.s, background: c.c }}>
                <c.Icon strokeWidth={2.2} />
              </div>
            </div>
          </div>
        </div>
      ))}
      <svg className="stage-svg">
        <line className="rail" x1="18" y1={RAIL_Y} x2="100%" y2={RAIL_Y} />
        <path className="branch" ref={branch} />
        <path className="rope" ref={rope} />
        <g ref={trolley}>
          <rect x="-15" y="-9" width="30" height="14" rx="5" className="f-lime" />
          <circle cx="-7" cy="-9" r="3.5" className="f-ink" />
          <circle cx="7" cy="-9" r="3.5" className="f-ink" />
        </g>
        <g className="fig" ref={fig}>
          {/* a hit area larger than the drawing, so it is easy to grab */}
          <rect x="-56" y="-8" width="112" height="150" fill="transparent" />
          <rect x="-4" y="-6" width="8" height="14" rx="4" className="s-rope" fill="none" strokeWidth="2.4" />
          <g ref={body}>
            <path d="M-11 112 L-11 127 M11 112 L11 127" className="s-coral-d" strokeWidth="6" strokeLinecap="round" />
            <ellipse cx="-13" cy="130" rx="8.5" ry="4.8" className="f-lime" />
            <ellipse cx="13" cy="130" rx="8.5" ry="4.8" className="f-lime" />
            <path d="M-30 36 C-30 8 30 8 30 36" className="s-sky" strokeWidth="5" fill="none" strokeLinecap="round" />
            <rect x="-28" y="18" width="56" height="96" rx="14" className="f-coral" />
            <rect x="-22" y="26" width="44" height="68" rx="8" className="f-screen" />
            <circle cx="0" cy="31" r="2" fill="#3a403c" />
            <g ref={eyes}>
              <ellipse cx="-9" cy="52" rx="5" ry="6.5" className="f-lime" />
              <ellipse cx="9" cy="52" rx="5" ry="6.5" className="f-lime" />
              <g ref={pupils}>
                <circle cx="-9" cy="53" r="2.5" className="f-ink" />
                <circle cx="9" cy="53" r="2.5" className="f-ink" />
              </g>
            </g>
            <circle cx="-15" cy="65" r="3" className="f-pink" opacity="0.8" />
            <circle cx="15" cy="65" r="3" className="f-pink" opacity="0.8" />
            <path d="M-6 67 Q0 73 6 67" className="s-lime" strokeWidth="2.2" fill="none" strokeLinecap="round" />
            <rect x="-7" y="102" width="14" height="3" rx="1.5" fill="#000" opacity="0.2" />
            <rect x="-36" y="28" width="9" height="18" rx="4" className="f-sky" />
            <rect x="27" y="28" width="9" height="18" rx="4" className="f-sky" />
            <g ref={arm}>
              <path d="M-28 62 Q-42 66 -46 80" className="s-coral-d" strokeWidth="6" fill="none" strokeLinecap="round" />
              <circle cx="-46" cy="82" r="4.5" className="f-coral-d" />
            </g>
            <path d="M28 62 Q40 68 40 79" className="s-coral-d" strokeWidth="6" fill="none" strokeLinecap="round" />
            <g ref={steam}>
              <path d="M38 74 q-3 -4 0 -8 q3 -4 0 -8" className="s-steam" strokeWidth="1.6" fill="none" strokeLinecap="round" />
              <path d="M44 74 q-3 -4 0 -8 q3 -4 0 -8" className="s-steam" strokeWidth="1.6" fill="none" strokeLinecap="round" />
            </g>
            <g ref={mug}>
              <path d="M48 81 q5.5 0 5.5 4.5 q0 4.5 -5.5 4.5" className="s-sun" strokeWidth="2.6" fill="none" />
              <rect x="34" y="77" width="15" height="14" rx="3" className="f-sun" />
            </g>
          </g>
        </g>
      </svg>
      <div className="bubble-pos" ref={bubblePos}>
        <div className="bubble" ref={bubble} />
      </div>
      <p className="stage-hint">Drag me, throw the chips at me, click to move my rail.</p>
    </aside>
  );
}
