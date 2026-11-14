"use client";

import { useRef, useState, type ComponentProps } from "react";
import { Check, Copy } from "lucide-react";

/** Code block with a copy control. The code is plain HTML; only the button needs script. */
export function CodeBlock(props: ComponentProps<"pre"> & { "data-language"?: string }) {
  const ref = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);
  const lang = props["data-language"];

  async function copy() {
    const text = ref.current?.innerText ?? "";
    try {
      await navigator.clipboard.writeText(text.replace(/\n$/, ""));
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      /* clipboard refused: the text stays selectable */
    }
  }

  return (
    <div className="codeblock not-prose">
      <div className="codeblock-head">
        <span>{lang && lang !== "text" && lang !== "plaintext" ? lang : "text"}</span>
        <button
          type="button"
          onClick={copy}
          className="inline-flex min-h-8 items-center gap-1.5 rounded-xs px-2 text-ink-2 transition-colors hover:bg-bg-alt hover:text-ink"
          aria-label={copied ? "Copied" : "Copy code"}
        >
          {copied ? <Check aria-hidden className="size-3.5" /> : <Copy aria-hidden className="size-3.5" />}
          <span className="font-sans text-xs">{copied ? "Copied" : "Copy"}</span>
        </button>
      </div>
      <pre ref={ref} {...props} tabIndex={0} />
    </div>
  );
}
