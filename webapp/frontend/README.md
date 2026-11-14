# Website (Phase 9)

The product website: home, download, documentation, requirements, troubleshooting, security,
privacy, licences, open source and releases. Next.js App Router with TypeScript, exported as a
static site (`output: "export"`), so it needs no server and no backend.

Everything here is isolated from the desktop app and the Runtime Core: its own `package.json`,
its own `node_modules`, its own scripts.

## Run

```bash
npm install
npm run dev        # http://localhost:27310
npm run build      # static site in out/
npm run lint
npm run typecheck
```

`predev` and `prebuild` run `scripts/sync-content.mjs`, which copies the user guides from
`docs/user/`, the security and contribution documents, and the MCP tool list
(`umr mcp --list-tools`, when a built `umr.exe` exists) into `content/generated/`. The docs pages
therefore always say what the repository says. Do not edit `content/generated/`.

## Releases on the download page

The download page reads the release manifest (`releases.json`, schema 1), the same file the app's
updater reads. By default it reads `content/releases.json`, which lists no release, so the page
shows "No public release yet". To preview a release layout locally, build with
`RELEASES_MANIFEST=<path to a releases.json>`; never ship a manifest whose URLs are not public.

## Layout

| Folder | What it holds |
|---|---|
| `app/` | Routes (App Router) and global CSS |
| `components/` | Site chrome, home sections, docs components, download, motion |
| `design-system/` | Colour, type and spacing tokens (`tokens.css`) and motion helpers (`motion.ts`) |
| `lib/` | Site facts and open decisions, docs registry, markdown rendering, release manifest |
| `content/` | Docs written for the site (`docs/`), the release manifest, generated copies |
| `scripts/` | Content sync |

## Rules

- Green-only brand palette; status colours only with an icon and a word.
- No trackers, analytics, cookies or third-party requests. Fonts are self-hosted.
- Animation with GSAP only where it explains something; `prefers-reduced-motion` is respected and
  every page works without JavaScript.
- Open decisions (name, domain, licence, hosting, telemetry, signing, PATH default, macOS/Linux)
  are shown as labelled placeholders, never guessed. See `lib/site.ts`.
