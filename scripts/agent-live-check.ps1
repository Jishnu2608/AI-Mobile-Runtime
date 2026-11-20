<#
.SYNOPSIS
  Phase 10.7 (CON-007): seeds or removes the live-check bug in a Flutter counter app, so every
  coding agent gets the same task: "tapping + does not increase the counter; find and fix it".

  powershell -File scripts\agent-live-check.ps1 -Project <flutter app>            # seed the bug
  powershell -File scripts\agent-live-check.ps1 -Project <flutter app> -Restore   # undo it
  powershell -File scripts\agent-live-check.ps1 -Project <flutter app> -Status    # seeded or not

  The bug: `_counter++;` in lib/main.dart becomes `_counter += int.parse("one");`, which throws a
  FormatException on every tap while the app keeps running. The runtime reports it as a runtime
  problem in the app's code with file and line; the agent should find it through the tumr tools,
  fix it, reload and tap + to verify. Works on the `flutter create` template.
#>
param(
  [Parameter(Mandatory = $true)][string]$Project,
  [switch]$Restore,
  [switch]$Status
)
$ErrorActionPreference = "Stop"
$file = Join-Path $Project "lib\main.dart"
if (-not (Test-Path $file)) { throw "$file not found: -Project must be a Flutter app folder" }
$text = [System.IO.File]::ReadAllText($file)
$good = "      _counter++;"
$bad = '      _counter += int.parse("one");'
$seeded = $text.Contains($bad)

if ($Status) { if ($seeded) { "seeded" } else { "clean" }; exit 0 }
if ($Restore) {
  if (-not $seeded) { "already clean"; exit 0 }
  [System.IO.File]::WriteAllText($file, $text.Replace($bad, $good), [System.Text.UTF8Encoding]::new($false))
  "restored: $file"
  exit 0
}
if ($seeded) { "already seeded"; exit 0 }
if (-not $text.Contains($good)) { throw "'_counter++;' not found in $file (not the counter template?)" }
[System.IO.File]::WriteAllText($file, $text.Replace($good, $bad), [System.Text.UTF8Encoding]::new($false))
"seeded: $file (tapping + now throws a FormatException)"
