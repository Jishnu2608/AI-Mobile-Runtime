import { AlertTriangle, CheckCircle2, CircleDashed, Info, XCircle } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export type StatusKind = "ok" | "warn" | "err" | "info" | "pending";

const icons = { ok: CheckCircle2, warn: AlertTriangle, err: XCircle, info: Info, pending: CircleDashed };
const classes = { ok: "badge-ok", warn: "badge-warn", err: "badge-err", info: "badge-info", pending: "" };

/** A state is never colour alone (directive §2.7): icon + word, always. */
export function Status({ kind, children, className }: { kind: StatusKind; children: ReactNode; className?: string }) {
  const Icon = icons[kind];
  return (
    <span className={cn("badge", classes[kind], className)}>
      <Icon aria-hidden className="size-3.5 shrink-0" />
      <span>{children}</span>
    </span>
  );
}
