# Privacy and security

- **Local only.** The runtime has no account, no cloud service and no telemetry. It contacts the internet only to download what you approved (from the official publishers), and, when you run `umr update`, the release manifest you configured. Android CLI is always run with `--no-metrics`.
- **Your system stays as it is.** Global `PATH`, `JAVA_HOME`, `ANDROID_HOME`, Android Studio and existing SDKs are never changed. Administrator rights are asked for only two named actions (Windows Hypervisor Platform, long paths), each with its own Windows prompt.
- **Verified downloads.** HTTPS from an allowlist of official hosts, checksums checked before anything is unpacked or run, archives checked for unsafe paths and links.
- **Project trust.** Building runs the project's own scripts (Gradle, npm, Flutter). Nothing builds until you trust the folder (`umr trust`); agents cannot trust folders.
- **Local API.** The Runtime Core listens on a named pipe only your Windows user can open, and every client needs a token. The preview uses a loopback connection with single-use tokens.
- **Agents.** Limited permissions, approvals for new folders and device switches, an audit log, and sanitised output — see [Agents](agents.md).
- **Logs.** Kept in `%USERPROFILE%\.umr\logs`; old ones are removed by `umr clean`.

Report a vulnerability as described in `SECURITY.md` in the repository. The latest security review is in `docs/security/`.
