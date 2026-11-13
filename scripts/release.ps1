<#
.SYNOPSIS
  Builds a release (REL-001): per-user NSIS installer, portable ZIP, SHA256SUMS and the release
  manifest (releases.json) under dist\<version>\. Signs the binaries and the installer when a
  code-signing certificate is configured; otherwise everything is unsigned and says so.

.PARAMETER BaseUrl
  Where the files will be published (the manifest's download URLs). Default: a placeholder that
  the website / release step replaces (Phase 9, DIST-001/002).
.PARAMETER Channel
  stable or beta.

.NOTES
  Signing: set UMR_SIGN_THUMBPRINT to the SHA-1 thumbprint of a code-signing certificate in the
  current user's certificate store (the private key never leaves the store), and optionally
  UMR_SIGN_TIMESTAMP_URL (default http://timestamp.digicert.com). Needs signtool.exe (Windows SDK).
  Run from the repository root. ASCII only (Windows PowerShell 5.1 reads scripts as ANSI).
#>
param(
  [string]$BaseUrl = "https://example.invalid/releases",
  [ValidateSet("stable", "beta")][string]$Channel = "stable",
  [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = (Get-Location).Path
$version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$out = Join-Path $root "dist\$version"
$maxBytes = 60MB   # NFR-05
$thumb = $env:UMR_SIGN_THUMBPRINT
$tsUrl = if ($env:UMR_SIGN_TIMESTAMP_URL) { $env:UMR_SIGN_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }

function Step($m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }

function Find-SignTool {
  $c = Get-Command signtool.exe -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
  $hit = Get-ChildItem $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName -Descending | Select-Object -First 1
  if ($hit) { return $hit.FullName }
  throw "UMR_SIGN_THUMBPRINT is set but signtool.exe was not found (install the Windows SDK)"
}

function Sign([string[]]$files) {
  if (-not $thumb) { return }
  $tool = Find-SignTool
  & $tool sign /sha1 $thumb /fd sha256 /tr $tsUrl /td sha256 /d "AI Mobile Runtime" @files
  if ($LASTEXITCODE -ne 0) { throw "signing failed" }
}

if (-not $SkipBuild) {
  Step "Third-party notices"
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\third-party-notices.ps1
  if ($LASTEXITCODE -ne 0) { throw "notices failed" }

  Step "Release binaries"
  # cargo can reuse a cached build carrying another version string (seen after the version went
  # back to one built before): rebuild the shipped packages from scratch
  cargo clean --release -p umr-core -p umr-cli -p umr-elevate -p umr-desktop
  cargo build --release --locked -p umr-core -p umr-cli -p umr-elevate
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
  Sign @("target\release\umr.exe", "target\release\umr-core.exe", "target\release\umr-elevate.exe")

  Step "Desktop UI"
  npm --prefix apps\desktop\ui ci --no-audit --no-fund
  npm --prefix apps\desktop\ui run build
  if ($LASTEXITCODE -ne 0) { throw "UI build failed" }

  Step "Installer (NSIS, per user)"
  Push-Location apps\desktop
  try {
    $cfg = @()
    if ($thumb) {
      $json = '{"bundle":{"windows":{"certificateThumbprint":"' + $thumb + '","digestAlgorithm":"sha256","timestampUrl":"' + $tsUrl + '"}}}'
      $cfg = @("--config", $json)
    }
    node ui\node_modules\@tauri-apps\cli\tauri.js build --bundles nsis @cfg
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
  } finally { Pop-Location }
}

Step "Versions"
foreach ($f in @("umr.exe", "umr-core.exe")) {
  $v = (& "target\release\$f" --version 2>&1 | Out-String).Trim()
  Write-Host "$f -> $v"
  if ($v -notmatch [regex]::Escape(" $version")) { throw "$f reports '$v', expected $version" }
}

Step "Collect into $out"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Force $out | Out-Null
$nsis = Get-ChildItem "target\release\bundle\nsis" -Filter "*_$($version)_x64-setup.exe" | Select-Object -First 1
if (-not $nsis) { throw "installer for $version not found under target\release\bundle\nsis" }
$installer = Join-Path $out "AI-Mobile-Runtime_$($version)_x64-setup.exe"
Copy-Item $nsis.FullName $installer

# portable ZIP: the same four programs; settings and toolchains go to %USERPROFILE%\.umr as usual
$stage = Join-Path $out "zip-stage\AI-Mobile-Runtime-$version"
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($f in @("umr.exe", "umr-core.exe", "umr-desktop.exe", "umr-elevate.exe")) { Copy-Item "target\release\$f" $stage }
Copy-Item THIRD_PARTY_NOTICES.md $stage
Copy-Item README.md $stage
$zip = Join-Path $out "AI-Mobile-Runtime_$($version)_x64-portable.zip"
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
Remove-Item (Join-Path $out "zip-stage") -Recurse -Force

# CLI only (CI / headless machines): umr + Core + elevation helper, no desktop app
$stage = Join-Path $out "cli-stage\umr-cli-$version"
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($f in @("umr.exe", "umr-core.exe", "umr-elevate.exe")) { Copy-Item "target\release\$f" $stage }
Copy-Item THIRD_PARTY_NOTICES.md $stage
$cli = Join-Path $out "umr-cli_$($version)_x64.zip"
Compress-Archive -Path $stage -DestinationPath $cli -CompressionLevel Optimal
Remove-Item (Join-Path $out "cli-stage") -Recurse -Force

Step "Checks"
$files = @(Get-Item $installer, $zip, $cli)
foreach ($f in $files) {
  $mb = [math]::Round($f.Length / 1MB, 1)
  Write-Host ("{0,-50} {1,6} MB" -f $f.Name, $mb)
  if ($f.Length -gt $maxBytes) { throw "$($f.Name) is larger than 60 MB (NFR-05)" }
}
$sig = Get-AuthenticodeSignature $installer
$signed = $sig.Status -eq "Valid"
Write-Host "installer signature: $($sig.Status)"
if ($thumb -and -not $signed) { throw "signing was requested but the installer signature is $($sig.Status)" }

Step "SHA256SUMS and releases.json"
$sums = foreach ($f in $files) { "{0}  {1}" -f (Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower(), $f.Name }
[System.IO.File]::WriteAllText((Join-Path $out "SHA256SUMS"), (($sums -join "`n") + "`n"), [System.Text.UTF8Encoding]::new($false))
$kinds = @{ $installer = "installer"; $zip = "portable"; $cli = "cli" }
$entries = foreach ($f in $files) {
  [ordered]@{
    kind   = $kinds[$f.FullName]
    name   = $f.Name
    url    = "$($BaseUrl.TrimEnd('/'))/v$version/$($f.Name)"
    size   = $f.Length
    sha256 = (Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower()
  }
}
$manifest = [ordered]@{
  schema   = 1
  product  = "AI Mobile Runtime"
  releases = @([ordered]@{
      version           = $version
      channel           = $Channel
      date              = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd")
      min_windows_build = 22000
      signed            = $signed
      notes_url         = "$($BaseUrl.TrimEnd('/'))/v$version/"
      files             = @($entries)
    })
}
[System.IO.File]::WriteAllText((Join-Path $out "releases.json"), ($manifest | ConvertTo-Json -Depth 6), [System.Text.UTF8Encoding]::new($false))
Get-ChildItem $out | Format-Table Name, Length -AutoSize
if (-not $signed) { Write-Warning "UNSIGNED build: SmartScreen will warn. Set UMR_SIGN_THUMBPRINT to sign (REL-001)." }
Write-Host "Release $version ready in $out"
