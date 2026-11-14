# Brand: the AI Mobile Runtime mark

The product mark (owner request, 2026-11-14): a phone whose screen shows a coding agent's prompt
`›_` — a mobile runtime for AI coding agents. Graphite & Lime, the website's palette.

| File | Use |
|---|---|
| `logo-mark.svg` | Master, 32 px and larger: website header and footer (inlined in `webapp/frontend/components/site/Wordmark.tsx`), desktop wizard header, large icon sizes |
| `logo-mark-small.svg` | Bold variant for 16–24 px: website favicon (`webapp/frontend/app/icon.svg`), small Windows icon sizes |
| `logo-1024.png` | 1024 px render of the master, for anything that needs a bitmap |

Colours: tile graphite gradient `#2C312E` → `#1A1D1B` (small variant: flat `#202321`), phone
`#F0F1ED` on `#202321`, prompt electric lime `#B9E769`. Keep the mark on its own tile; do not
recolour the prompt.

The name next to it is still the working name (open decision W1).

## Windows icon
`apps/desktop/icons/icon.ico` holds PNG images at 16, 20, 24, 32 (small variant) and 40, 48, 64,
128, 256 px (master). To rebuild it after changing the artwork: render each SVG at those sizes
with a transparent background (any headless browser), then pack the PNGs into one ICO
(little-endian `ICONDIR` + one `ICONDIRENTRY` per size, PNG data appended). Windows Vista and
later read PNG entries.
