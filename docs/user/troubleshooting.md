# Troubleshooting

Start with `umr problems` (or ☰ in the preview). Each problem has a **category** and an **origin**: *your code* means fix the project; *environment* means the toolchain, the device or the network.

## Build (your code)
Compiler, resource or bundler errors with file and line — Kotlin, Java, Android resources (AAPT), Dart, Babel/Metro. Fix the file; Flutter and React Native reload by themselves, Kotlin/Java: `umr reload full`. Errors under `build/`, `node_modules/` or the pub cache come from generated or third-party code: check versions before editing them.

## Runtime and crash
- **Runtime**: the app keeps running but reported an error (Flutter "EXCEPTION CAUGHT BY …", "Unhandled Exception", JavaScript errors). The location points to the widget or the first frame in your code.
- **Crash**: the app process died (Java/Kotlin exception, native crash, "app not responding"). `umr problems show <fingerprint>` shows the stack; `umr logs --source logcat --level E` the app's last log lines.

## Environment
| Message contains | Cause | Fix |
|---|---|---|
| `SDK location not found`, `Failed to find target`, `Failed to find Platform SDK` | Android SDK package missing | `umr plan .` then `umr install .` |
| `licences have not been accepted` | A licence not accepted yet | `umr install .` shows it |
| `requires Java 17`, `Unsupported class file major version` | JDK does not fit the Gradle / AGP version | `umr plan .` picks the right JDK |
| `Java heap space`, `daemon disappeared` | Gradle ran out of memory | close other apps; add `org.gradle.jvmargs=-Xmx4g` to `gradle.properties` |
| `address already in use` / port 8081 | Another Metro or dev server runs | stop it, or let `umr run` pick another port |
| `hardware acceleration`, `WHPX` | Virtualization off | [Requirements](requirements.md) |
| `filename … too long` | Windows path limit | `umr admin enable-long-paths`, or a shorter project path |
| `is not trusted yet` | Folder not trusted | `umr trust .` |

## Network
`Could not GET …`, `UnknownHostException`, `PKIX path building failed`, `ENOTFOUND`, `Could not connect to development server`: a download or the dev server could not be reached. Check the connection and proxy; for TLS-inspecting company proxies, their root certificate must be in the Windows certificate store.

## Device
`device offline`, `unauthorized`, `INSTALL_FAILED_INSUFFICIENT_STORAGE`, `INSTALL_FAILED_UPDATE_INCOMPATIBLE`: `umr session` shows the device; `umr device use <profile> --cold` reboots it cleanly; `umr app uninstall <package>` removes an old copy with another signature.

## Still stuck?
`umr doctor` checks the machine. When reporting an issue, attach `umr doctor --json`, `umr problems --json` and the newest file in `%USERPROFILE%\.umr\logs`. Problem and log output from `umr` is already sanitised; check raw log files before sharing them.
