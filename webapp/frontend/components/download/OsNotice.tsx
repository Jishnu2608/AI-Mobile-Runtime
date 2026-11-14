"use client";

import { useEffect, useState } from "react";
import { Status } from "@/components/ui/Status";

type Seen = "windows11" | "windows" | "mac" | "linux" | "mobile" | "unknown";

type UAData = { platform?: string; mobile?: boolean; getHighEntropyValues?: (h: string[]) => Promise<{ platformVersion?: string; architecture?: string }> };

async function detect(): Promise<Seen> {
  const nav = navigator as Navigator & { userAgentData?: UAData };
  const ua = navigator.userAgent;
  const d = nav.userAgentData;
  if (d?.mobile || /Android|iPhone|iPad/i.test(ua)) return "mobile";
  if (d?.platform === "Windows" && d.getHighEntropyValues) {
    try {
      const v = await d.getHighEntropyValues(["platformVersion"]);
      // Windows 11 reports platformVersion 13 or later
      return Number((v.platformVersion ?? "0").split(".")[0]) >= 13 ? "windows11" : "windows";
    } catch {
      return "windows";
    }
  }
  if (/Windows/i.test(ua)) return "windows";
  if (/Mac OS X|Macintosh/i.test(ua)) return "mac";
  if (/Linux|X11/i.test(ua)) return "linux";
  return "unknown";
}

/**
 * Says whether this computer can run the runtime. Only reads the browser's own platform hints;
 * nothing is stored or sent. Without script the static requirement line above stays as it is.
 * What macOS and Linux visitors are offered beyond this notice is open decision W10 (DEC-043).
 */
export function OsNotice() {
  const [seen, setSeen] = useState<Seen | null>(null);

  useEffect(() => {
    detect().then(setSeen);
  }, []);

  if (!seen || seen === "unknown") return null;
  const text: Record<Exclude<Seen, "unknown">, [kind: "ok" | "info" | "warn", label: string, body: string]> = {
    windows11: ["ok", "Windows 11 detected", "This computer meets the operating system requirement. Check the hardware list before you install."],
    windows: ["info", "Windows detected", "Your browser does not say which Windows version this is. The runtime needs Windows 11 x64 (build 22000 or later)."],
    mac: ["warn", "macOS detected", "The runtime runs on Windows 11 x64 only today. macOS support is not planned in this release."],
    linux: ["warn", "Linux detected", "The runtime runs on Windows 11 x64 only today. Linux support is not planned in this release."],
    mobile: ["info", "Phone or tablet detected", "The runtime is a Windows desktop program. Open this page on your Windows 11 PC to download it."],
  };
  const [kind, label, body] = text[seen];
  return (
    <div role="status" className="mt-6 flex flex-col gap-2 rounded-md border border-line bg-surface px-4 py-3 sm:flex-row sm:items-center sm:gap-4">
      <Status kind={kind} className="shrink-0 self-start whitespace-nowrap sm:self-auto">{label}</Status>
      <p className="text-sm text-ink-2">{body}</p>
    </div>
  );
}
