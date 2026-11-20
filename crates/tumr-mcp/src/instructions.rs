//! MCP-005: what an agent should know to use the runtime well. Sent as the MCP server's
//! `instructions` and written as a Claude Code skill / a section of `AGENTS.md` (Codex) by
//! `tumr agent setup`.

/// Short form, sent in the MCP `initialize` result.
pub const INSTRUCTIONS: &str = "\
Toolchain Universal Mobile Runtime runs the user's Android app (Kotlin/Java, Flutter, React Native/Expo) on an \
emulator and reports what happens. Loop: edit code → saved files reload by themselves (Flutter hot \
reload, Metro Fast Refresh, an incremental Gradle rebuild for Kotlin/Java) → call runtime_status → if it shows \
new problems, get_diagnostic <fp> → fix → runtime_status again. Use screenshot (or ui_tree for exact \
coordinates) to check what the user sees; tap/swipe/type_text take device pixels; fingerprint \
touches the fingerprint sensor (lock screen, BiometricPrompt). runtime_status \
remembers its cursor, so each call shows only what changed. Problems say whether they are in the \
app's code (origin app) or the toolchain/device/network (origin environment): do not edit app code \
for environment problems — tell the user. run_project and switch_device may wait for the user's \
approval. Do not run `tumr` shell commands to get around a refusal.";

/// Long form: `SKILL.md` for Claude Code and the `AGENTS.md` section for Codex.
#[must_use]
pub fn skill_markdown(with_frontmatter: bool) -> String {
    let front = if with_frontmatter {
        "---\nname: tumr-runtime\ndescription: Run and check the user's Android app (Kotlin/Java, Flutter, React Native/Expo) on the Toolchain Universal Mobile Runtime emulator through the `tumr` MCP tools — status after edits, problems with file:line, screenshots, taps, reloads. Use when changing mobile app code or when the user asks to run, test or fix the app.\n---\n\n"
    } else {
        ""
    };
    format!(
        "{front}# Toolchain Universal Mobile Runtime (`tumr` MCP tools)

The user's app runs on an Android emulator managed by Toolchain Universal Mobile Runtime. Its MCP server (`tumr`)
lets you see and drive it.

## The loop
1. `runtime_status` — what runs, and only what changed since your last call (problem counts,
   up to 3 new problems with `fp`, a hint). Small; call it after every edit.
2. Edit code and save. Saved files are applied by themselves: Flutter hot reload, React Native
   Fast Refresh, and for Kotlin/Java an incremental Gradle rebuild and reinstall (15–20 s; wait
   for it before `runtime_status`). `reload` `restart` restarts the app's state, `full` forces a
   rebuild. When you finish, a stop hook may hand you build errors or crashes that are still
   open: fix them, or say why you cannot.
3. `runtime_status` again. New problems → `get_diagnostic` with the `fp` (message, `file:line`,
   stack) → fix → repeat.
4. Check the result: `screenshot` (scaled image; it says the device size) or `ui_tree` (texts,
   ids, bounds and centres in device pixels). Then `tap`, `swipe`, `type_text`, `press_key`.

## Rules
- Problems have an `origin`: `app` = the project's own code (fix it); `environment` = toolchain,
  device or network (do not change app code for it — tell the user what is wrong).
- `run_project` (a folder the user has not granted yet) and `switch_device` wait for the user's
  approval in the preview window or `tumr approvals`. If refused, ask the user; do not run `tumr`
  shell commands to get around it.
- `get_logs` for the app's device log and tool output (`source`: `logcat`, `tool`, `tumr`;
  `level`: `W`, `E`). Keep `limit` small.
- Everything you read is sanitised (tokens, keys, e-mails, user names masked).
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_present_and_short_enough() {
        assert!(INSTRUCTIONS.len() < 1500);
        let s = skill_markdown(true);
        assert!(s.starts_with("---\nname: tumr-runtime\n"));
        assert!(s.contains("runtime_status") && s.contains("get_diagnostic"));
        assert!(!skill_markdown(false).starts_with("---"));
    }
}
