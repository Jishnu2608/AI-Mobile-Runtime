import Link from "next/link";
import { ArrowRight, BookOpen, Download, FileSearch, Lock, MonitorSmartphone, PlugZap, ShieldCheck, Wrench } from "lucide-react";
import { Reveal } from "@/components/motion/Reveal";
import { Status } from "@/components/ui/Status";
import { Workflow } from "./Workflow";
import { Architecture } from "./Architecture";
import { stages } from "./workflow-data";
import { pendingDecisions } from "@/lib/site";

function SectionHead({ id, eyebrow, title, lead }: { id: string; eyebrow: string; title: string; lead?: string }) {
  return (
    // the eyebrow is kept as the section's accessible description only; a heading that names
    // the thing needs no visible label over it
    <header className="max-w-3xl" data-reveal data-section={eyebrow}>
      <h2 id={id} className="t-section">
        {title}
      </h2>
      {lead && <p className="t-lead mt-5">{lead}</p>}
    </header>
  );
}

/* ---------------------------------------------------------------- pillars */

const pillars = [
  {
    icon: Wrench,
    title: "Toolchain from the project",
    body: "It reads the Gradle wrapper, Flutter and Node constraints, then installs only what is missing: JDK, Android SDK, emulator, Flutter, Node. Sizes and licences are shown before anything downloads.",
    fact: "No administrator rights for the installs",
  },
  {
    icon: MonitorSmartphone,
    title: "An emulator that stays out of the way",
    body: "Phones boot headless from a snapshot on the runtime's own adb server, so Android Studio and your existing SDKs keep working. Six phone and tablet profiles, from compact to tablet.",
    fact: "Global PATH, JAVA_HOME and ANDROID_HOME untouched",
  },
  {
    icon: PlugZap,
    title: "Your app beside your agent",
    body: "The running app streams into a window you dock next to Claude Code or Codex. You tap and type in it; the agent sees the same screen through screenshots and the UI tree.",
    fact: "Flutter hot reload 0.7 to 1.8 s, measured",
  },
  {
    icon: FileSearch,
    title: "Problems an agent can act on",
    body: "Build errors, crashes and runtime exceptions are classified, deduplicated and located at file and line, and marked as your code or the environment.",
    fact: "Tested on 76 seeded failures across six categories",
  },
];

export function Pillars() {
  return (
    <section aria-labelledby="pillars-title" className="section border-t border-line">
      <Reveal className="wrap">
        <SectionHead
          id="pillars-title"
          eyebrow="What it does"
          title="Four jobs, one local runtime"
          lead="Everything an agent needs to see a mobile app run, without you learning the Android toolchain first."
        />
        <ul className="mt-14 grid gap-px overflow-hidden rounded-lg border border-line bg-line sm:grid-cols-2">
          {pillars.map((p) => (
            <li key={p.title} data-reveal className="flex flex-col bg-surface p-6 sm:p-8">
              <p.icon aria-hidden className="size-6 text-pine dark:text-jade" strokeWidth={1.75} />
              <h3 className="t-sub mt-5">{p.title}</h3>
              <p className="t-body mt-3 flex-1">{p.body}</p>
              <p className="mt-6 border-t border-line pt-4 text-sm font-medium text-ink">{p.fact}</p>
            </li>
          ))}
        </ul>
      </Reveal>
    </section>
  );
}

/* ---------------------------------------------------- recording placeholder */

/** WEB-003: a real screen recording is not made yet. The frame says so instead of faking a UI. */
export function RecordingSlot() {
  return (
    <section aria-labelledby="recording-title" className="section pt-0">
      <div className="wrap">
        <figure className="relative grid min-h-[18rem] w-full place-items-center py-12 sm:aspect-[21/9] sm:min-h-0 overflow-hidden rounded-lg border border-dashed border-line-strong bg-bg-alt">
          <div className="max-w-md px-6 text-center">
            <Status kind="pending">Recording pending</Status>
            <h2 id="recording-title" className="t-sub mt-5">
              A screen recording of the docked preview goes here
            </h2>
            <p className="t-desc mt-3">
              It will show a real session: a Flutter app running beside Claude Code, an error, the fix and the reload. Nothing in this
              frame is a mock-up.
            </p>
          </div>
        </figure>
      </div>
    </section>
  );
}

/* ----------------------------------------------------------- how it works */

export function HowItWorks() {
  return (
    <section aria-labelledby="how-title" className="section border-t border-line bg-bg-alt">
      <Reveal className="wrap">
        <SectionHead
          id="how-title"
          eyebrow="How it works"
          title="From a folder to a fixed bug in eight stages"
          lead="Pick a stage to see what runs, who runs it and which parts take part. Stages one to five are yours; six to eight are the agent's loop."
        />
        <div className="mt-14" data-reveal>
          <Workflow />
          <noscript>
            <ol className="mt-8 grid gap-4">
              {stages.map((s, i) => (
                <li key={s.id} className="rounded-md border border-line bg-surface p-5">
                  <p className="t-label">Stage {i + 1}</p>
                  <h3 className="t-sub mt-1">{s.title}</h3>
                  <p className="t-body mt-2">{s.body}</p>
                  <pre className="mt-3 font-mono text-sm">{s.command}</pre>
                </li>
              ))}
            </ol>
          </noscript>
        </div>
      </Reveal>
    </section>
  );
}

/* ------------------------------------------------------------- frameworks */

const frameworks = [
  { name: "Kotlin and Java", build: "Gradle, with the project's own wrapper", reload: "Full rebuild and reinstall (umr reload full)", kind: "warn" as const, detect: "build.gradle(.kts) with an Android application" },
  { name: "Flutter", build: "flutter run in machine mode", reload: "Hot reload on save, hot restart on request", kind: "ok" as const, detect: "pubspec.yaml with an android/ folder" },
  { name: "React Native", build: "Gradle for native code, Metro for JavaScript", reload: "Fast Refresh on save", kind: "ok" as const, detect: "package.json with react-native" },
  { name: "Expo", build: "Expo Go, or a native development build", reload: "Fast Refresh on save", kind: "ok" as const, detect: "package.json with expo" },
];

export function Frameworks() {
  return (
    <section aria-labelledby="frameworks-title" className="section border-t border-line">
      <Reveal className="wrap">
        <SectionHead
          id="frameworks-title"
          eyebrow="Frameworks"
          title="Kotlin, Java, Flutter, React Native and Expo"
          lead="One adapter per framework. Each uses the framework's own tools, so a project that builds in Android Studio or the Flutter CLI builds here."
        />
        <div className="table-wrap mt-12" data-reveal role="region" aria-label="Framework support" tabIndex={0}>
          <table className="w-full min-w-[44rem] border-collapse text-left text-[0.95rem]">
            <thead>
              <tr className="border-b border-line-strong">
                <th scope="col" className="t-label py-3 pr-6 font-medium">Framework</th>
                <th scope="col" className="t-label py-3 pr-6 font-medium">Detected by</th>
                <th scope="col" className="t-label py-3 pr-6 font-medium">Builds with</th>
                <th scope="col" className="t-label py-3 font-medium">After you save</th>
              </tr>
            </thead>
            <tbody>
              {frameworks.map((f) => (
                <tr key={f.name} className="border-b border-line align-top">
                  <th scope="row" className="py-4 pr-6 font-semibold text-ink">{f.name}</th>
                  <td className="py-4 pr-6 text-ink-2">{f.detect}</td>
                  <td className="py-4 pr-6 text-ink-2">{f.build}</td>
                  <td className="py-4">
                    <Status kind={f.kind}>{f.reload}</Status>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="t-desc mt-6" data-reveal>
          Framework guides:{" "}
          <Link className="link" href="/docs/guides/kotlin-java/">Kotlin and Java</Link>,{" "}
          <Link className="link" href="/docs/guides/flutter/">Flutter</Link>,{" "}
          <Link className="link" href="/docs/guides/react-native-expo/">React Native and Expo</Link>.
        </p>
      </Reveal>
    </section>
  );
}

/* ----------------------------------------------------------------- agents */

type ToolLite = { name: string; annotations: { readOnlyHint: boolean } };

export function Agents({ tools }: { tools: ToolLite[] }) {
  const reads = tools.filter((t) => t.annotations.readOnlyHint);
  const acts = tools.filter((t) => !t.annotations.readOnlyHint);
  return (
    <section aria-labelledby="agents-title" className="section band">
      <Reveal className="wrap grid gap-12 lg:grid-cols-12 lg:gap-10">
        <div className="lg:col-span-5">
          <h2 id="agents-title" className="t-section" data-reveal>
            Claude Code and Codex, connected over MCP
          </h2>
          <p className="t-lead mt-5" data-reveal>
            One command writes the agent&apos;s MCP entry and a short skill file. It shows you the exact change first and keeps your other
            servers.
          </p>
          <div className="mt-8 grid gap-3" data-reveal>
            {["umr agent setup claude-code", "umr agent setup codex"].map((c) => (
              <pre key={c} className="overflow-x-auto rounded-md border border-band-line bg-band-sunken px-4 py-3 font-mono text-sm text-mint">
                {c}
              </pre>
            ))}
          </div>
          <Link href="/docs/guides/agents/" className="btn btn-primary mt-8 h-11 px-5" data-reveal>
            Read the agent guide
            <ArrowRight aria-hidden className="btn-icon size-4" />
          </Link>
        </div>
        <div className="grid gap-5 lg:col-span-7" data-reveal>
          <ToolGroup title={`Reads only · ${reads.length} tools`} note="Status, problems, logs, the screen and the UI tree." tools={reads} />
          <ToolGroup title={`Acts on the device or app · ${acts.length} tools`} note="Taps, typing, rotation, reload, run and stop. A device switch asks you first." tools={acts} />
          <p className="text-sm text-band-ink-2">
            Every agent gets its own token, limited permissions and an audit log. Output is sanitised before the agent sees it.
          </p>
        </div>
      </Reveal>
    </section>
  );
}

function ToolGroup({ title, note, tools }: { title: string; note: string; tools: ToolLite[] }) {
  return (
    <div className="rounded-lg border border-band-line bg-band-surface p-5 sm:p-6">
      <h3 className="font-semibold text-band-ink">{title}</h3>
      <p className="mt-1 text-sm text-band-ink-2">{note}</p>
      <ul className="mt-4 flex flex-wrap gap-2">
        {tools.map((t) => (
          <li key={t.name}>
            <Link
              href={`/docs/reference/mcp/#${t.name}`}
              className="inline-flex min-h-8 items-center rounded-sm border border-band-line bg-band-sunken px-2.5 font-mono text-[0.8125rem] text-band-ink transition-colors hover:border-band-accent hover:text-band-accent"
            >
              {t.name}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}

/* ----------------------------------------------------------- architecture */

export function ArchitectureSection() {
  return (
    <section aria-labelledby="arch-title" className="section border-t border-line">
      <div className="wrap">
        <Reveal>
          <SectionHead
            id="arch-title"
            eyebrow="Architecture"
            title="A Runtime Core that outlives every window"
            lead="The command line, the desktop app and the agent are clients of one Core per Windows user. Close the app or restart the agent, and the emulator and the app keep running."
          />
        </Reveal>
        <div className="mt-14">
          <Architecture />
        </div>
      </div>
    </section>
  );
}

/* ---------------------------------------------------------------- privacy */

const privacy = [
  { icon: Lock, title: "Local only", body: "No account, no cloud service, no telemetry. It downloads only what you approved, from the official publishers." },
  { icon: ShieldCheck, title: "Verified downloads", body: "HTTPS from an allowlist of official hosts. Checksums are checked before anything is unpacked or run." },
  { icon: Wrench, title: "Your system as it was", body: "Global PATH, JAVA_HOME, ANDROID_HOME and existing SDKs are never changed. Two named actions may ask for administrator rights." },
  { icon: FileSearch, title: "Project trust", body: "Building runs the project's own scripts. Nothing builds until you trust the folder, and agents cannot trust folders." },
];

export function Privacy() {
  return (
    <section aria-labelledby="privacy-title" className="section border-t border-line bg-bg-alt">
      <Reveal className="wrap">
        <SectionHead id="privacy-title" eyebrow="Privacy and security" title="Your code and your machine stay yours" />
        <ul className="mt-12 grid gap-6 sm:grid-cols-2 lg:grid-cols-4">
          {privacy.map((p) => (
            <li key={p.title} data-reveal>
              <p.icon aria-hidden className="size-5 text-pine dark:text-jade" strokeWidth={1.75} />
              <h3 className="mt-4 font-semibold text-ink">{p.title}</h3>
              <p className="t-desc mt-2">{p.body}</p>
            </li>
          ))}
        </ul>
        <div className="mt-12 flex flex-wrap gap-3" data-reveal>
          <Link href="/privacy/" className="btn h-11 px-5">Read the privacy notes</Link>
          <Link href="/security/" className="btn btn-ghost h-11 px-5">See the security review</Link>
        </div>
      </Reveal>
    </section>
  );
}

/* ------------------------------------------------------------ open source */

export function OpenSource() {
  return (
    <section aria-labelledby="oss-title" className="section border-t border-line">
      <Reveal className="wrap grid gap-10 lg:grid-cols-12">
        <div className="lg:col-span-6">
          <SectionHead
            id="oss-title"
            eyebrow="Open source"
            title="The licence is not chosen yet"
            lead="The source repository stays private until the product licence is decided. The third-party licences the runtime ships with are already listed."
          />
        </div>
        <ul className="grid content-start gap-3 lg:col-span-5 lg:col-start-8" data-reveal>
          {pendingDecisions
            .filter((d) => ["W1", "W3", "W5"].includes(d.w))
            .map((d) => (
              <li key={d.w} className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-line bg-surface px-4 py-3">
                <span className="font-medium text-ink">{d.what}</span>
                <Status kind="pending">Pending decision</Status>
              </li>
            ))}
          <li className="pt-3">
            <Link href="/open-source/#open-decisions" className="link font-medium">
              See every open decision
            </Link>
            <span className="text-ink-2"> · </span>
            <Link href="/licences/" className="link font-medium">
              Third-party licences
            </Link>
          </li>
        </ul>
      </Reveal>
    </section>
  );
}

/* -------------------------------------------------------------------- FAQ */

const faq = [
  { q: "Does it replace Android Studio?", a: "No. It runs next to it. It uses its own adb server and its own SDK folder, so Android Studio and your existing SDKs keep working as before." },
  { q: "Can it run iOS apps?", a: "No. The iOS Simulator needs macOS and Xcode. This runtime is for Windows 11 x64 and Android today." },
  { q: "Which coding agents does it work with?", a: "Claude Code and Codex have one-command setup. Any agent that speaks MCP over stdio can use the umr mcp server by hand." },
  { q: "Does my code leave my machine?", a: "No. The runtime has no cloud service. It downloads tools from their official publishers and nothing else, unless you run umr update." },
  { q: "What does it need from my PC?", a: "Windows 11 x64, virtualisation turned on in the firmware, and room for the toolchain and one phone image. The requirements page lists each level." },
  { q: "How do I remove it?", a: "Uninstall it from Windows Settings. The uninstall guide lists every folder it created and how to remove the downloaded tools too." },
];

export function Faq() {
  return (
    <section aria-labelledby="faq-title" className="section border-t border-line">
      <Reveal className="wrap grid gap-10 lg:grid-cols-12">
        <div className="lg:col-span-4">
          <SectionHead id="faq-title" eyebrow="Questions" title="Before you install" />
        </div>
        <div className="divide-y divide-line border-y border-line lg:col-span-8">
          {faq.map((f) => (
            <details key={f.q} className="group" data-reveal>
              <summary className="flex min-h-14 cursor-pointer list-none items-center justify-between gap-6 py-4 text-[1.0625rem] font-medium text-ink [&::-webkit-details-marker]:hidden">
                {f.q}
                <span aria-hidden className="grid size-7 shrink-0 place-items-center rounded-full border border-line text-ink-2 transition-transform group-open:rotate-45">
                  +
                </span>
              </summary>
              <p className="t-body max-w-2xl pb-5">{f.a}</p>
            </details>
          ))}
        </div>
      </Reveal>
    </section>
  );
}

/* -------------------------------------------------------------- close CTA */

export function Close({ hasRelease }: { hasRelease: boolean }) {
  return (
    <section aria-labelledby="close-title" className="section band">
      <Reveal className="wrap text-center">
        <h2 id="close-title" className="t-section mx-auto max-w-[18ch]" data-reveal>
          Give your agent a phone to test on
        </h2>
        <p className="t-lead mx-auto mt-5 max-w-xl" data-reveal>
          Windows 11 x64. Kotlin, Java, Flutter, React Native and Expo.
        </p>
        <div className="mt-9 flex flex-wrap justify-center gap-3" data-reveal>
          <Link href="/download/" className="btn btn-primary h-12 px-5 text-base">
            <Download aria-hidden className="size-4" />
            {hasRelease ? "Download for Windows" : "See downloads"}
          </Link>
          <Link href="/docs/getting-started/" className="btn h-12 border-band-line bg-transparent px-5 text-base text-band-ink hover:bg-band-surface">
            <BookOpen aria-hidden className="size-4" />
            Read the getting-started guide
          </Link>
        </div>
      </Reveal>
    </section>
  );
}
