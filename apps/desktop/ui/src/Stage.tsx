/**
 * UI-007: the playful right-hand panel of the setup window. A little phone character ("Pocket")
 * hangs from a rail by a rope: drag it and fling it, click anywhere in the panel and the rail
 * trolley carries it there, click it and it reacts. Floating developer chips drift behind it.
 * Purely decorative (aria-hidden); it says one short line when the wizard step changes.
 * The rope is a small position-based pendulum; GSAP does the trolley, the chips and the small
 * character animations. The loop sleeps when everything is at rest.
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
};

const POKES = ["Wheee!", "Hot reload me again!", "I run on coffee and Gradle.", "Careful, I'm in debug mode.", "No crashes on my watch.", "Again! Again!", "Build successful. Probably."];

const CHIPS: { Icon: LucideIcon; c: string; x: number; y: number; s: number; d: number; say: string }[] = [
  { Icon: Braces, c: "var(--lime)", x: 10, y: 30, s: 50, d: 1.2, say: "Curly braces: my favourite hugs." },
  { Icon: Bug, c: "var(--c-coral)", x: 76, y: 20, s: 46, d: 0.8, say: "A bug! The Problems tab will catch it." },
  { Icon: Coffee, c: "var(--c-sun)", x: 18, y: 62, s: 44, d: 1.0, say: "Refill, please." },
  { Icon: Cog, c: "var(--c-sky)", x: 80, y: 50, s: 52, d: 1.4, say: "Gradle is thinking. Give it a second." },
  { Icon: GitBranch, c: "var(--c-pink)", x: 8, y: 86, s: 40, d: 0.6, say: "Commit early, commit often." },
  { Icon: Terminal, c: "var(--lime)", x: 58, y: 80, s: 42, d: 0.9, say: "umr run — that's the whole trick." },
  { Icon: Play, c: "var(--c-sky)", x: 84, y: 82, s: 38, d: 0.7, say: "Press play, watch it go." },
  { Icon: Smartphone, c: "var(--c-sun)", x: 40, y: 92, s: 36, d: 0.5, say: "That's my cousin." },
];

const RAIL_Y = 26;
const SCALE = 1.25;
const G = 1900;

export function Stage({ step }: { step: string }) {
  const root = useRef<HTMLElement>(null);
  const rope = useRef<SVGPathElement>(null);
  const trolley = useRef<SVGGElement>(null);
  const fig = useRef<SVGGElement>(null);
  const body = useRef<SVGGElement>(null);
  const eyes = useRef<SVGGElement>(null);
  const pupils = useRef<SVGGElement>(null);
  const arm = useRef<SVGGElement>(null);
  const steam = useRef<SVGGElement>(null);
  const bubblePos = useRef<HTMLDivElement>(null);
  const bubble = useRef<HTMLDivElement>(null);
  const api = useRef<{ say(text: string, delay?: number): void; wave(): void } | null>(null);
  const first = useRef(true);

  useEffect(() => {
    const el = root.current!;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const ctx = gsap.context(() => undefined, el);
    const add = (f: () => void) => ctx.add(f);

    let W = el.clientWidth || 400;
    let H = el.clientHeight || 800;
    let L = Math.max(150, H * 0.34);
    const a = { x: W * 0.5 };
    const p = reduce ? { x: a.x, y: RAIL_Y + L } : { x: a.x + 110, y: RAIL_Y + 20 };
    const v = { x: 0, y: 0 };
    let rot = 0;
    let targetRot = 0;
    let bw = 0;
    let raf = 0;
    let last = 0;
    let still = 0;
    let anchorTween: gsap.core.Tween | null = null;
    let hide: gsap.core.Tween | null = null;
    type Drag = { ox: number; oy: number; sx: number; sy: number; moved: boolean; t: number; vx: number; vy: number };
    let drag: Drag | null = null;
    let down: { x: number; y: number } | null = null;
    let onPar: ((e: PointerEvent) => void) | null = null;

    const clamp = gsap.utils.clamp;

    function constrain() {
      const dx = p.x - a.x;
      const dy = p.y - RAIL_Y;
      const d = Math.hypot(dx, dy);
      if (d > L) {
        p.x = a.x + (dx / d) * L;
        p.y = RAIL_Y + (dy / d) * L;
      }
      p.x = clamp(46, W - 46, p.x);
      p.y = clamp(RAIL_Y + 12, H - 190, p.y);
    }

    function physics(dt: number) {
      if (drag) return;
      const px = p.x;
      const py = p.y;
      const damp = Math.exp(-0.85 * dt);
      v.x *= damp;
      v.y = v.y * damp + G * dt;
      p.x += v.x * dt;
      p.y += v.y * dt;
      constrain();
      v.x = (p.x - px) / dt;
      v.y = (p.y - py) / dt;
    }

    function render() {
      const dx = p.x - a.x;
      const dy = p.y - RAIL_Y;
      const d = Math.hypot(dx, dy);
      const slack = L - d;
      rope.current?.setAttribute(
        "d",
        slack > 2
          ? `M${a.x} ${RAIL_Y} Q${(a.x + p.x) / 2} ${(RAIL_Y + p.y) / 2 + slack * 0.7} ${p.x} ${p.y}`
          : `M${a.x} ${RAIL_Y} L${p.x} ${p.y}`,
      );
      trolley.current?.setAttribute("transform", `translate(${a.x} ${RAIL_Y})`);
      const hang = d > 1 ? (-Math.atan2(dx, dy) * 180) / Math.PI : 0;
      targetRot = slack > 2 ? hang * Math.max(0, 1 - slack / 50) : hang;
      rot += (targetRot - rot) * 0.22;
      fig.current?.setAttribute("transform", `translate(${p.x.toFixed(1)} ${p.y.toFixed(1)}) rotate(${rot.toFixed(2)}) scale(${SCALE})`);
      if (bubblePos.current) {
        const right = p.x + 84 + bw < W - 12;
        const x = right ? p.x + 84 : Math.max(12, p.x - 84 - bw);
        bubblePos.current.style.transform = `translate(${x.toFixed(1)}px, ${(p.y + 28).toFixed(1)}px)`;
      }
    }

    function frame(t: number) {
      const dt = clamp(0.004, 0.033, (t - last) / 1000 || 0.016);
      last = t;
      for (let i = 0; i < 4; i++) physics(dt / 4);
      render();
      const moving = drag || anchorTween?.isActive() || Math.hypot(v.x, v.y) > 3 || Math.abs(targetRot - rot) > 0.15;
      still = moving ? 0 : still + 1;
      if (still > 30) {
        raf = 0;
        return;
      }
      raf = requestAnimationFrame(frame);
    }
    function wake() {
      still = 0;
      if (!raf) {
        last = performance.now();
        raf = requestAnimationFrame(frame);
      }
    }

    function moveTo(x: number) {
      anchorTween?.kill();
      add(() => {
        anchorTween = gsap.to(a, { x: clamp(46, W - 46, x), duration: 1.1, ease: "power2.inOut", onUpdate: wake });
      });
      wake();
    }

    function say(text: string, delay = 0) {
      if (!text) return;
      add(() => {
        gsap.delayedCall(delay, () => {
          const b = bubble.current;
          if (!b) return;
          b.textContent = text;
          bw = b.offsetWidth;
          render();
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

    function poke() {
      say(POKES[Math.floor(Math.random() * POKES.length)]);
      wave();
      if (!reduce && body.current) {
        add(() => {
          gsap.fromTo(body.current, { scaleX: 1.16, scaleY: 0.84 }, { scaleX: 1, scaleY: 1, duration: 0.7, ease: "elastic.out(1, 0.35)", transformOrigin: "50% 0%" });
        });
      }
      v.x += (Math.random() < 0.5 ? -1 : 1) * gsap.utils.random(350, 600);
      v.y -= 250;
      wake();
    }

    api.current = { say, wave };

    // pointer: drag the character, click a chip, click anywhere else to move the trolley
    const local = (e: PointerEvent) => {
      const r = el.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const onDown = (e: PointerEvent) => {
      const t = e.target as Element;
      const { x, y } = local(e);
      if (t.closest(".fig")) {
        anchorTween?.kill();
        drag = { ox: p.x - x, oy: p.y - y, sx: x, sy: y, moved: false, t: performance.now(), vx: 0, vy: 0 };
        el.classList.add("dragging");
        wake();
        e.preventDefault();
        return;
      }
      const chip = t.closest<HTMLElement>(".chip");
      if (chip) {
        const c = CHIPS[Number(chip.dataset.i)];
        if (!reduce) add(() => gsap.fromTo(chip, { scale: 0.7, rotation: -25 }, { scale: 1, rotation: 0, duration: 0.8, ease: "elastic.out(1.1, 0.4)" }));
        say(c.say);
        return;
      }
      down = { x, y };
    };
    const onMove = (e: PointerEvent) => {
      // the eyes follow the pointer anywhere in the window
      const r = el.getBoundingClientRect();
      const ex = r.left + p.x;
      const ey = r.top + p.y + 52 * SCALE;
      const dx = e.clientX - ex;
      const dy = e.clientY - ey;
      const d = Math.hypot(dx, dy) || 1;
      const k = Math.min(2.6, d / 60) / d;
      pupils.current?.setAttribute("transform", `translate(${(dx * k).toFixed(2)} ${(dy * k).toFixed(2)})`);
      if (!drag) return;
      const { x, y } = local(e);
      if (!drag.moved && Math.hypot(x - drag.sx, y - drag.sy) > 5) drag.moved = true;
      const nx = x + drag.ox;
      const ny = y + drag.oy;
      const now = performance.now();
      const dt = Math.max(8, now - drag.t) / 1000;
      drag.vx = drag.vx * 0.5 + ((nx - p.x) / dt) * 0.5;
      drag.vy = drag.vy * 0.5 + ((ny - p.y) / dt) * 0.5;
      drag.t = now;
      p.x = nx;
      p.y = ny;
      // pulled past the rope's length: the trolley follows along the rail
      const ddx = p.x - a.x;
      const ddy = p.y - RAIL_Y;
      if (Math.hypot(ddx, ddy) > L) {
        const reach = Math.sqrt(Math.max(0, L * L - ddy * ddy));
        a.x = clamp(46, W - 46, p.x - Math.sign(ddx) * reach);
      }
      constrain();
      wake();
    };
    const onUp = (e: PointerEvent) => {
      if (drag) {
        if (drag.moved) {
          v.x = clamp(-2600, 2600, drag.vx);
          v.y = clamp(-2600, 2600, drag.vy);
        } else poke();
        drag = null;
        el.classList.remove("dragging");
        wake();
        return;
      }
      if (down) {
        const { x, y } = local(e);
        if (Math.hypot(x - down.x, y - down.y) < 6) moveTo(x);
        down = null;
      }
    };
    el.addEventListener("pointerdown", onDown);
    window.addEventListener("pointermove", onMove, { passive: true });
    window.addEventListener("pointerup", onUp);

    const ro = new ResizeObserver(() => {
      W = el.clientWidth;
      H = el.clientHeight;
      L = Math.max(150, H * 0.34);
      a.x = clamp(46, W - 46, a.x);
      constrain();
      wake();
    });
    ro.observe(el);

    render();
    if (!reduce) {
      wake(); // the drop-in entrance: it falls onto its rope and swings
      add(() => {
        el.querySelectorAll<HTMLElement>(".chip-float").forEach((f) => {
          gsap.to(f, { y: gsap.utils.random(-16, 16), rotation: gsap.utils.random(-12, 12), duration: gsap.utils.random(2.4, 4.4), yoyo: true, repeat: -1, ease: "sine.inOut", delay: gsap.utils.random(0, 1.2) });
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
          if (!drag && !document.hidden) {
            v.x += gsap.utils.random(-110, 110);
            wake();
          }
          add(() => gsap.delayedCall(gsap.utils.random(6, 11), breeze));
        };
        gsap.delayedCall(7, breeze);
      });
      // parallax: the chips lean away from the pointer, deeper ones more
      const movers = Array.from(el.querySelectorAll<HTMLElement>(".chip-pos")).map((n) => ({
        d: Number(n.dataset.d),
        x: gsap.quickTo(n, "x", { duration: 0.9, ease: "power3" }),
        y: gsap.quickTo(n, "y", { duration: 0.9, ease: "power3" }),
      }));
      onPar = (e: PointerEvent) => {
        const fx = e.clientX / window.innerWidth - 0.5;
        const fy = e.clientY / window.innerHeight - 0.5;
        movers.forEach((m) => {
          m.x(-fx * 26 * m.d);
          m.y(-fy * 18 * m.d);
        });
      };
      window.addEventListener("pointermove", onPar, { passive: true });
    }

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      el.removeEventListener("pointerdown", onDown);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      if (onPar) window.removeEventListener("pointermove", onPar);
      api.current = null;
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
          <div className="chip-float">
            <div className="chip" data-i={i} style={{ width: c.s, height: c.s, background: c.c }}>
              <c.Icon strokeWidth={2.2} />
            </div>
          </div>
        </div>
      ))}
      <svg className="stage-svg">
        <line className="rail" x1="18" y1={RAIL_Y} x2="100%" y2={RAIL_Y} />
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
            <path d="M48 81 q5.5 0 5.5 4.5 q0 4.5 -5.5 4.5" className="s-sun" strokeWidth="2.6" fill="none" />
            <rect x="34" y="77" width="15" height="14" rx="3" className="f-sun" />
          </g>
        </g>
      </svg>
      <div className="bubble-pos" ref={bubblePos}>
        <div className="bubble" ref={bubble} />
      </div>
      <p className="stage-hint">Drag me, or click anywhere here to move my rail.</p>
    </aside>
  );
}
