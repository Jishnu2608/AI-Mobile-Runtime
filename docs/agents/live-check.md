# Live check of a coding-agent connection (Phase 10.7, CON-007)

The same task for every agent, so results can be compared. A client counts as **supported**
only after it passes this check (DEC-046 / DEC-054).

## Task
A Flutter counter app (`flutter create` template) where tapping **+** does nothing, because
`scripts/agent-live-check.ps1` changed `_counter++;` into `_counter += int.parse("one");`.

Prompt, word for word:

> This Flutter app has a bug: something goes wrong when the user taps the + button. Use the umr
> MCP tools (the AI Mobile Runtime) to run the app on the emulator, reproduce the problem, find its
> cause with file and line, fix the code, reload, and verify on the device that tapping +
> increases the counter. Do not use a shell.

Pass: the agent uses the `umr` tools, edits `lib/main.dart` back to a working increment, reloads,
and shows (screenshot or UI tree) that the counter goes up.

## Steps
1. `powershell -File scripts\agent-live-check.ps1 -Project <app>` — seeds the bug.
2. `umr agent setup <agent> --project <app>` (Claude Desktop: no `--project`), then
   `umr agent doctor <agent> --project <app>` must show only `ok`.
3. Optional, so nothing waits for an approval: `umr agent grant <agent> <app>` and
   `umr agent grant <agent> device:switch`.
4. Start the agent in `<app>` (Claude Desktop: a new chat), give the prompt.
5. Record: tools called (`umr agent audit --client <agent>`), number of turns, time, result.
6. `powershell -File scripts\agent-live-check.ps1 -Project <app> -Status` must say `clean`;
   otherwise `-Restore`.

## Results

| Agent | Date | Result | Turns / time | Notes |
|---|---|---|---|---|
| Claude Code 2.1.310 (headless `claude -p`) | 2026-11-14 | **Pass** | 30 turns, 248 s, $0.69 | Connected through the `.mcp.json` written by `umr agent setup`; skill loaded. Found `lib/main.dart:66`, fixed it, verified 0 → 2. Exposed two runtime bugs: `run_project` attached to an older installed build (the bug only showed after a restart), and the tap-handler `FormatException` was not reported as a problem (PRV-015, DIA-007) |
| Cursor | — | not run yet | | connected and doctor-checked; run by the owner |
| VS Code (agent mode) | — | not run yet | | connected and doctor-checked; run by the owner |
| Claude Desktop | — | not run yet | | connected (user config) and doctor-checked; run by the owner |
| Codex | — | not run | | not installed on this PC (MCP-007) |
