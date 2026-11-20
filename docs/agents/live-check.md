# Live check of a coding-agent connection (Phase 10.7, CON-007)

The same task for every agent, so results can be compared. A client counts as **supported**
only after it passes this check (DEC-046 / DEC-054).

## Task
A Flutter counter app (`flutter create` template) where tapping **+** does nothing, because
`scripts/agent-live-check.ps1` changed `_counter++;` into `_counter += int.parse("one");`.

Prompt, word for word:

> This Flutter app has a bug: something goes wrong when the user taps the + button. Use the tumr
> MCP tools (the Toolchain Universal Mobile Runtime) to run the app on the emulator, reproduce the problem, find its
> cause with file and line, fix the code, reload, and verify on the device that tapping +
> increases the counter. Do not use a shell.

Pass: the agent uses the `tumr` tools, edits `lib/main.dart` back to a working increment, reloads,
and shows (screenshot or UI tree) that the counter goes up.

## Steps
1. `powershell -File scripts\agent-live-check.ps1 -Project <app>` — seeds the bug.
2. `tumr agent setup <agent> --project <app>` (Claude Desktop: no `--project`), then
   `tumr agent doctor <agent> --project <app>` must show only `ok`.
3. Optional, so nothing waits for an approval: `tumr agent grant <agent> <app>` and
   `tumr agent grant <agent> device:switch`.
4. Start the agent in `<app>` (Claude Desktop: a new chat), give the prompt.
5. Record: tools called (`tumr agent audit --client <agent>`), number of turns, time, result.
6. `powershell -File scripts\agent-live-check.ps1 -Project <app> -Status` must say `clean`;
   otherwise `-Restore`.

## Results

| Agent | Date | Result | Turns / time | Notes |
|---|---|---|---|---|
| Claude Code 2.1.310 (headless `claude -p`) | 2026-11-14 | **Pass** | 30 turns, 248 s, $0.69 | Connected through the `.mcp.json` written by `tumr agent setup`; skill loaded. Found `lib/main.dart:66`, fixed it, verified 0 → 2. Exposed two runtime bugs: `run_project` attached to an older installed build (the bug only showed after a restart), and the tap-handler `FormatException` was not reported as a problem (PRV-015, DIA-007) |
| Claude Code 2.1.310, stop hook (HOOK-001) | 2026-11-15 | **Pass** | 8 turns, 42 s, $0.31 | Kotlin demo with a typo seeded (`uppercse`); the prompt asked only for a KDoc comment. The first stop was blocked with `DataRepository.kt:14 Unresolved reference 'uppercse'`; Claude called `runtime_status`, fixed the typo; the hook waited for the rebuild (17 s) and let it stop. With an ambiguous error (`platformName()` undefined) it explained and stopped after one report |
| Claude Code 2.1.310 | 2026-11-19 | **Pass** | 37 turns, 103 s (Kotlin); 24 turns, 37 s (Flutter) | Kotlin compile error: `runtime_status` → fix → rebuild → `ui_tree` / taps, 12 + 3 = 15. Flutter `FormatException` on 1 ÷ 3 =: reproduced by tapping, `get_diagnostic`, hot reload, 0.333333333333 |
| Codex CLI 0.169 (headless `codex exec`) | 2026-11-19 | **Pass** (MCP loop) | — | Java compile error and a React Native render error: `runtime_status` → `get_diagnostic` → fix → rebuild / Fast Refresh → `screenshot`, `ui_tree`, taps; `switch_device` after an approval. Its stop hook runs only after you trust it once (`/hooks`) |
| Cursor (owner) | 2026-11-19 | **Pass** | — | React Native render error, side task: `runtime_status` twice; the stop hook's report (`followup_message`) made Cursor fix `App.tsx` 16 s later |
| VS Code agent mode, Copilot (owner) | 2026-11-19 | **Pass** | — | Error hidden in `src/calculator.ts`: `runtime_status`, `diagnostics.list`, `get_diagnostic`, `screenshot`, fixed line 8, confirmed with `ui_tree` and taps |
| Antigravity App (owner) | 2026-11-19 | **Pass** | — | `runtime_status`; the Stop hook said continue with the report and Antigravity re-entered its loop |
| Claude Desktop | — | not run | | A running Claude Desktop writes its settings back and drops the `tumr` entry: quit it completely before connecting (desktop_emulator #49) |
