import { CircleDot, FolderGit2, ShieldCheck, ThumbsUp } from "lucide-react";
import { inline } from "./inline";

type Level = "required" | "recommended" | "optional" | "project";
type Row = { item: string; need: string; level: Level; notes?: string };

const levels: Record<Level, { label: string; icon: typeof ShieldCheck; cls: string }> = {
  required: { label: "Required", icon: ShieldCheck, cls: "badge-lime" },
  recommended: { label: "Recommended", icon: ThumbsUp, cls: "badge-info" },
  optional: { label: "Optional", icon: CircleDot, cls: "" },
  project: { label: "Per project", icon: FolderGit2, cls: "badge-info" },
};

export function RequirementTable({ rows, caption }: { rows: Row[]; caption: string }) {
  return (
    <div className="not-prose my-6 overflow-x-auto rounded-md border border-line bg-surface" role="region" aria-label={caption} tabIndex={0}>
      <table className="w-full min-w-[34rem] border-collapse text-[0.925rem] leading-relaxed">
        <caption className="sr-only">{caption}</caption>
        <thead>
          <tr className="bg-sunken text-left">
            <th scope="col" className="px-4 py-2.5 font-semibold text-ink">What</th>
            <th scope="col" className="px-4 py-2.5 font-semibold text-ink">Needed</th>
            <th scope="col" className="px-4 py-2.5 font-semibold text-ink">Level</th>
            <th scope="col" className="px-4 py-2.5 font-semibold text-ink">Notes</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const L = levels[r.level];
            const Icon = L.icon;
            return (
              <tr key={r.item} className="border-t border-line align-top">
                <th scope="row" className="px-4 py-3 text-left font-medium text-ink">{r.item}</th>
                <td className="px-4 py-3 text-ink">{inline(r.need)}</td>
                <td className="px-4 py-3">
                  <span className={`badge ${L.cls}`}>
                    <Icon aria-hidden className="size-3.5" />
                    {L.label}
                  </span>
                </td>
                <td className="px-4 py-3 text-ink-2">{r.notes ? inline(r.notes) : null}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
