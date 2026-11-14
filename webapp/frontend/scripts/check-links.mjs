// After `next build`: every internal link and #anchor in the static export must resolve, and no
// page may load a third-party script, stylesheet or font. Exit code 1 lists what is wrong.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const out = join(process.cwd(), "out");
if (!existsSync(out)) {
  console.error("check-links: out/ not found, run `npm run build` first");
  process.exit(1);
}

const pages = [];
(function walk(d) {
  for (const n of readdirSync(d)) {
    const p = join(d, n);
    if (statSync(p).isDirectory()) walk(p);
    else if (n.endsWith(".html")) pages.push(p);
  }
})(out);

const decode = (s) => s.replace(/&amp;/g, "&").replace(/&#x27;/g, "'").replace(/&quot;/g, '"');
const ids = new Map(pages.map((p) => [p, new Set([...readFileSync(p, "utf8").matchAll(/\sid="([^"]+)"/g)].map((m) => decode(m[1])))]));
const problems = [];

for (const page of pages) {
  const html = readFileSync(page, "utf8");
  const rel = page.slice(out.length + 1);
  for (const m of html.matchAll(/<(?:script|link)\b[^>]*(?:src|href)="(https?:)?\/\/([^"/]+)/g)) {
    problems.push(`${rel}: third-party resource from ${m[2]}`);
  }
  for (const m of html.matchAll(/href="([^"]+)"/g)) {
    const href = decode(m[1]);
    if (/^(https?:|mailto:)/.test(href) || href.startsWith("/_next/")) continue;
    if (href.startsWith("#")) {
      if (href.length > 1 && !ids.get(page).has(href.slice(1))) problems.push(`${rel}: missing anchor ${href}`);
      continue;
    }
    if (!href.startsWith("/")) continue;
    const [pathAndQuery, frag] = href.split("#");
    const path = pathAndQuery.split("?")[0];
    let target = join(out, path);
    if (path.endsWith("/")) target = join(target, "index.html");
    if (!existsSync(target)) {
      problems.push(`${rel}: broken link ${href}`);
      continue;
    }
    if (frag && ids.has(target) && !ids.get(target).has(frag)) problems.push(`${rel}: missing anchor ${href}`);
  }
}

const unique = [...new Set(problems)];
if (unique.length) {
  console.error(`check-links: ${unique.length} problem(s)\n  ${unique.join("\n  ")}`);
  process.exit(1);
}
console.log(`check-links: ${pages.length} pages, all internal links and anchors resolve, no third-party resources`);
