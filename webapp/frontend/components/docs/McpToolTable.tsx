import { readFileSync } from "node:fs";
import { join } from "node:path";
import { Status } from "@/components/ui/Status";

type Tool = {
  name: string;
  title: string;
  description: string;
  inputSchema: { properties?: Record<string, { type?: string; description?: string; enum?: string[] }>; required?: string[] };
  annotations: { readOnlyHint: boolean };
};

/** The tool catalogue exactly as `umr mcp --list-tools` prints it (content/generated). */
export function loadTools(): Tool[] {
  return JSON.parse(readFileSync(join(process.cwd(), "content", "generated", "mcp-tools.json"), "utf8"));
}

export function McpToolTable() {
  const tools = loadTools();
  return (
    <div className="not-prose my-8 divide-y divide-line overflow-hidden rounded-md border border-line bg-surface">
      {tools.map((t) => {
        const props = Object.entries(t.inputSchema.properties ?? {});
        const required = new Set(t.inputSchema.required ?? []);
        return (
          <section key={t.name} id={t.name} className="scroll-mt-24 px-5 py-5 sm:px-6">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <h3 className="font-mono text-[0.95rem] font-semibold text-ink">{t.name}</h3>
              {t.annotations.readOnlyHint ? <Status kind="info">reads only</Status> : <Status kind="pending">acts on the device or app</Status>}
            </div>
            <p className="mt-2 text-[0.95rem] leading-relaxed text-ink-2">{t.description}</p>
            {props.length > 0 && (
              <dl className="mt-3 grid gap-x-6 gap-y-1.5 text-sm sm:grid-cols-[max-content_1fr]">
                {props.map(([k, v]) => (
                  <div key={k} className="contents">
                    <dt className="font-mono text-ink">
                      {k}
                      <span className="text-ink-2">{required.has(k) ? "" : "?"}</span>
                      <span className="ml-2 text-ink-2">{v.enum ? v.enum.join(" | ") : v.type}</span>
                    </dt>
                    <dd className="text-ink-2">{v.description ?? (required.has(k) ? "required" : "optional")}</dd>
                  </div>
                ))}
              </dl>
            )}
          </section>
        );
      })}
    </div>
  );
}
