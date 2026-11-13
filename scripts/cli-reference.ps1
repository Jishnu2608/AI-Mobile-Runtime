<#
.SYNOPSIS
  Writes docs/user/cli-reference.md from the `umr` binary's own help (REL-004), so the reference
  never drifts from the code. Run from the repository root after building:
  powershell -File scripts\cli-reference.ps1 [-Umr target\release\umr.exe]
#>
param([string]$Umr = "target\release\umr.exe", [string]$Out = "docs\user\cli-reference.md")
$ErrorActionPreference = "Stop"
if (-not (Test-Path $Umr)) { throw "$Umr not found; build first (cargo build --release -p umr-cli)" }

function Help([string[]]$cmd) {
  $text = & $Umr @cmd --help 2>&1 | Out-String
  return $text.TrimEnd()
}

function Subcommands([string]$help) {
  $in = $false
  foreach ($l in $help -split "`r?`n") {
    if ($l -match '^Commands:') { $in = $true; continue }
    if ($in -and $l -match '^\S') { break }
    if ($in -and $l -match '^\s{2}([a-z][a-z0-9-]*)\s') {
      if ($Matches[1] -ne "help") { $Matches[1] }
    }
  }
}

$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add("# ``umr`` command reference")
$lines.Add("")
$lines.Add("Generated from ``umr --help`` by ``scripts/cli-reference.ps1`` - do not edit by hand.")
$lines.Add("")
$top = Help @()
$lines.Add('```text'); $lines.Add($top); $lines.Add('```')
foreach ($c in Subcommands $top) {
  $h = Help @($c)
  $lines.Add(""); $lines.Add("## umr $c"); $lines.Add("")
  $lines.Add('```text'); $lines.Add($h); $lines.Add('```')
  foreach ($s in Subcommands $h) {
    $lines.Add(""); $lines.Add("### umr $c $s"); $lines.Add("")
    $lines.Add('```text'); $lines.Add((Help @($c, $s))); $lines.Add('```')
  }
}
$text = (($lines -join "`n") -replace 'umr\.exe', 'umr') + "`n"
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
[System.IO.File]::WriteAllText((Join-Path (Get-Location) $Out), $text, [System.Text.UTF8Encoding]::new($false))
Write-Host "$Out written"
