# Security policy

## Reporting a vulnerability
Please **do not** open a public issue. Use GitHub's private vulnerability reporting: the repository's **Security** tab → **Report a vulnerability**. Include the version (`umr --version`), what an attacker needs (local user, another local process, a web page, a malicious project, a malicious agent, a network position), steps to reproduce, and the impact.

You will get an acknowledgement within 7 days. We aim to fix high and critical issues before the next release and will credit you in the release notes unless you prefer otherwise.

## Supported versions
Only the latest release receives security fixes.

## Scope
In scope: the Runtime Core, `umr`, the desktop app, the elevation helper, the installer and updater, the MCP server, and how they handle downloads, archives, the local API, the preview connection, project trust, agent permissions and logs.

Out of scope: vulnerabilities in third-party tools the runtime downloads at the user's request (Android SDK, emulator, JDK, Flutter, Node, Gradle) — report those to their publishers; and actions of an agent that the user allowed to run arbitrary shell commands (see the threat model).

## Design and review
The threat model (T1–T12) and the latest review are in [`docs/security/`](docs/security/review-v1.md). Releases list their SHA-256 checksums; code signing is planned.
