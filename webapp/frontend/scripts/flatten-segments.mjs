// After `next build`: the static export writes nested route-segment prefetch files as folders
// (`download/__next.download/__PAGE__.txt`), but the App Router requests them as one dotted name
// (`download/__next.download.__PAGE__.txt`). A plain static host then answers 404 and the router
// loses its prefetch. This copies each nested file to its dotted name, so any static host works
// without rewrite rules (hosting is open decision W5). The folders are kept.
import { copyFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

const out = join(process.cwd(), "out");
let copied = 0;

function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (!statSync(p).isDirectory()) continue;
    if (name.startsWith("__next.")) flatten(p, dir);
    else walk(p);
  }
}

function flatten(segDir, parent) {
  const stack = [segDir];
  while (stack.length) {
    const d = stack.pop();
    for (const name of readdirSync(d)) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) stack.push(p);
      else if (name.endsWith(".txt")) {
        const dotted = relative(parent, p).split(sep).join(".");
        copyFileSync(p, join(parent, dotted));
        copied++;
      }
    }
  }
}

walk(out);
console.log(`flatten-segments: ${copied} prefetch files copied to their dotted names`);
