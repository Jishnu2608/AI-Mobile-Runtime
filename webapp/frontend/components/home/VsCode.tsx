import Link from "next/link";
import { ArrowRight, Puzzle } from "lucide-react";
import { Reveal } from "@/components/motion/Reveal";
import { Status } from "@/components/ui/Status";
import { site } from "@/lib/site";

/* The VS Code extension on the home page. */

const features = [
  { title: "Run on Emulator", body: "From the Command Palette or the status bar: checks the toolchain, boots the emulator, builds, installs and launches the open folder." },
  { title: "Mobile Preview panel", body: "The live phone beside your code: click and type, rotate, screenshot, device sizes, and the in-display fingerprint sensor." },
  { title: "Saved files apply themselves", body: "Flutter hot reload, React Native Fast Refresh, an incremental Gradle rebuild for Kotlin and Java." },
  { title: "Problems panel", body: "Build errors, runtime errors and crashes of the running app, with file and line, next to your compiler's own." },
  { title: "Copilot agent mode", body: "Connect Copilot gives VS Code's agent the runtime's tools: status, problems, screenshot, tap, reload, fingerprint." },
];

/** The store button: a link once the extension is published, a labelled placeholder until then. */
export function MarketplaceButton({ className = "" }: { className?: string }) {
  const m = site.vscode.marketplace;
  if (m.published) {
    return (
      <a href={m.url} className={`btn btn-primary h-12 px-5 text-base ${className}`} rel="noopener">
        <Puzzle aria-hidden className="size-4" />
        Get it on the VS Code Marketplace
      </a>
    );
  }
  return (
    <span className={`btn h-12 cursor-not-allowed px-5 text-base opacity-70 ${className}`} aria-disabled="true" title="Not published yet">
      <Puzzle aria-hidden className="size-4" />
      VS Code Marketplace · coming soon
    </span>
  );
}

export function VsCode() {
  return (
    <section aria-labelledby="vscode-title" className="section border-t border-line" id="vscode">
      <Reveal className="wrap grid items-center gap-12 lg:grid-cols-12 lg:gap-10">
        <div className="lg:col-span-6">
          <header data-reveal>
            <h2 id="vscode-title" className="t-section">
              Your Android app inside VS Code
            </h2>
            <p className="t-lead mt-5">
              One extension for Kotlin, Java, Flutter and React Native: run the app on the emulator, see the phone in a panel beside your
              code, and get its errors where your compiler&apos;s already are.
            </p>
          </header>
          <ul className="mt-8 grid gap-4" data-reveal>
            {features.map((f) => (
              <li key={f.title} className="flex gap-3">
                <span aria-hidden className="mt-2 size-1.5 shrink-0 rounded-full bg-accent" />
                <span>
                  <span className="font-semibold text-ink">{f.title}.</span> <span className="text-ink-2">{f.body}</span>
                </span>
              </li>
            ))}
          </ul>
          <div className="mt-9 flex flex-wrap items-center gap-3" data-reveal>
            <MarketplaceButton />
            <Link href="/docs/guides/vscode/" className="btn h-12 px-5 text-base">
              Read the VS Code guide
              <ArrowRight aria-hidden className="btn-icon size-4" />
            </Link>
          </div>
          <p className="t-desc mt-4" data-reveal>
            Needs the runtime installed. Until the Marketplace listing is live, install the <code>.vsix</code> as described on the{" "}
            <Link className="link" href="/download/#vscode">download page</Link>. Try it with the{" "}
            <a className="link" href={`${site.repository}/tree/main/sample-app`} rel="noopener">
              sample calculators
            </a>{" "}
            in Kotlin, Java, Dart and React Native.
          </p>
        </div>
        <div className="lg:col-span-6" data-reveal>
          <EditorSketch />
        </div>
      </Reveal>
    </section>
  );
}

/**
 * A drawn sketch of the layout (editor, Mobile Preview panel, Problems panel), not a screenshot:
 * a real capture replaces it later.
 */
function EditorSketch() {
  return (
    <figure aria-label="Sketch: VS Code with the Mobile Preview panel beside the editor" className="vs-sketch">
      <div className="vs-titlebar">
        <span />
        <span />
        <span />
        <p>Calculator.kt — sample-kotlin</p>
      </div>
      <div className="vs-body">
        <div className="vs-editor" aria-hidden>
          {[62, 80, 44, 70, 90, 38, 76, 58, 84, 50, 66, 40].map((w, i) => (
            <div key={i} className={i === 6 ? "vs-line vs-line-error" : "vs-line"} style={{ width: `${w}%` }} />
          ))}
        </div>
        <div className="vs-panel" aria-hidden>
          <p className="vs-panel-title">Mobile Preview</p>
          <div className="vs-phone">
            <span className="vs-cam" />
            <div className="vs-display">60</div>
            <div className="vs-keys">
              {Array.from({ length: 16 }, (_, i) => (
                <span key={i} className={i % 4 === 3 ? "vs-key vs-key-op" : "vs-key"} />
              ))}
            </div>
          </div>
        </div>
      </div>
      <div className="vs-problems">
        <Status kind="warn">Problems 1</Status>
        <code>Calculator.kt:53 · Unresolved reference &apos;bb&apos; · Toolchain Universal Mobile Runtime · build</code>
      </div>
      <div className="vs-statusbar">
        <span>▣ com.tumr.samples.kotlincalc</span>
        <span>emulator-5554</span>
      </div>
      <figcaption className="sr-only">
        The editor on the left, the running app in the Mobile Preview panel on the right, and the runtime&apos;s build error in the
        Problems panel below.
      </figcaption>
    </figure>
  );
}
