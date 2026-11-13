# Failure corpus (TEST-006)

Seeded failures for the diagnostics pipeline (Phase 5, exit M5: ≥ 90 % correct category,
≥ 80 % correct file:line). Run by `crates/umr-diagnostics/tests/corpus.rs`
(`cargo test -p umr-diagnostics --test corpus -- --nocapture` prints the table).

Each `*.txt` file holds cases:

```text
=== short name
kind: gradle | flutter-build | flutter-log | metro | tool | crash | anr | install | missing | message
source: <tool name, for tool / message>
fallback: <category, for message; default environment>
expect: environment | build | runtime | crash | device | network
origin: app | environment | unknown        (optional, reported, not gated)
at: <file:line> | none                     (optional; counted for the file:line rate when not none)
---
<the tool's output, as it prints it>
```

The runner feeds each text through the same code the product runs: `problems::parse_line` and
`gradle_failure` (Gradle / Flutter / Metro output, first problem = the one that counts),
`problems::flutter_exception` (Flutter framework errors in the app log), `parse_crash_buffer`
and `parse_anr` (device), adapter errors (`install`, `missing`), and `from_message` for text
from the device and session layers. Project root for paths: `C:\p`.

The texts follow the real tools' output formats; project, class and file names are made up.
Write new cases from real failures before changing rules, and keep cases that fail: the rate
is the point.
