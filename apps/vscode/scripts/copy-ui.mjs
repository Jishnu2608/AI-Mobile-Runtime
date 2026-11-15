// Copies the desktop app's built UI (the same Preview component the desktop window shows) into
// media/ui, where the Mobile Preview panel loads it from. Build the UI first:
//   npm --prefix ../desktop/ui run build
import { cpSync, existsSync, readdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const from = join(here, "..", "..", "desktop", "ui", "dist", "assets");
const to = join(here, "..", "media", "ui");
if (!existsSync(from)) {
  console.error(`missing ${from}: run \`npm --prefix apps/desktop/ui run build\` first`);
  process.exit(1);
}
rmSync(to, { recursive: true, force: true });
cpSync(from, to, { recursive: true });
console.log(`copied ${readdirSync(to).length} files to media/ui`);
