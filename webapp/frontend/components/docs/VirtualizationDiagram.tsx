/**
 * The four layers the emulator's speed depends on, bottom to top, with the check for each.
 * A technical diagram, not a screenshot: every label is a real setting or command.
 */
const layers = [
  { name: "Android system image (x86_64)", detail: "installed by the runtime for your project", check: "umr plan ." },
  { name: "Android Emulator, accelerated", detail: "uses Windows Hypervisor Platform", check: "umr doctor" },
  { name: "Windows Hypervisor Platform", detail: "a Windows feature; the app can turn it on", check: "umr admin enable-whpx" },
  { name: "CPU virtualization (VT-x / AMD-V)", detail: "a firmware (BIOS/UEFI) setting", check: "Task Manager → Performance → CPU" },
];

export function VirtualizationDiagram() {
  return (
    <figure className="not-prose my-8" aria-label="Virtualization layers the emulator needs, from firmware at the bottom to the Android image at the top">
      <ol className="flex flex-col gap-2">
        {layers.map((l, i) => (
          <li
            key={l.name}
            className="grid items-center gap-x-5 gap-y-1 rounded-md border border-line bg-surface px-5 py-3.5 sm:grid-cols-[1fr_auto] sm:[margin-inline:var(--inset)]"
            style={{ ["--inset" as string]: `${(layers.length - 1 - i) * 1.25}rem` }}
          >
            <div>
              <p className="font-medium text-ink">
                <span className="mr-2 font-mono text-xs text-ink-2">{layers.length - i}</span>
                {l.name}
              </p>
              <p className="text-sm text-ink-2">{l.detail}</p>
            </div>
            <code className="w-fit rounded-xs border border-line bg-[var(--code-bg)] px-2 py-1 font-mono text-xs text-ink">{l.check}</code>
          </li>
        ))}
      </ol>
    </figure>
  );
}
