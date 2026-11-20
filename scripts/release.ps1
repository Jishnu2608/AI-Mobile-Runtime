<#
.SYNOPSIS
  Builds a release (REL-001): per-user NSIS installer, portable ZIP, SHA256SUMS and the release
  manifest (releases.json) under dist\<version>\. Signs the binaries and the installer when a
  code-signing certificate is configured; otherwise everything is unsigned and says so.

.PARAMETER BaseUrl
  Where the files will be published (the manifest's download URLs are <BaseUrl>/v<version>/<file>).
  For GitHub Releases (W5) pass https://github.com/<owner>/<repo>/releases/download; the release
  notes link then points at .../releases/tag/v<version>. Default: a placeholder.
.PARAMETER Channel
  stable or beta.
.PARAMETER SkipVscode
  Do not build the VS Code extension (.vsix) into the release.

.NOTES
  Signing: set TUMR_SIGN_THUMBPRINT to the SHA-1 thumbprint of a code-signing certificate in the
  current user's certificate store (the private key never leaves the store), and optionally
  TUMR_SIGN_TIMESTAMP_URL (default http://timestamp.digicert.com). Needs signtool.exe (Windows SDK).
  Run from the repository root. ASCII only (Windows PowerShell 5.1 reads scripts as ANSI).
#>
param(
  [string]$BaseUrl = "https://example.invalid/releases",
  [ValidateSet("stable", "beta")][string]$Channel = "stable",
  [switch]$SkipBuild,
  [switch]$SkipVscode
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = (Get-Location).Path
$version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$out = Join-Path $root "dist\$version"
$maxBytes = 60MB   # NFR-05
$thumb = $env:TUMR_SIGN_THUMBPRINT
$tsUrl = if ($env:TUMR_SIGN_TIMESTAMP_URL) { $env:TUMR_SIGN_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }

# GitHub Releases (W5): files under .../releases/download/v<version>/, notes on .../releases/tag/v<version>
$base = $BaseUrl.TrimEnd('/')
$notesUrl = if ($base -match '/releases/download$') { ($base -replace '/releases/download$', '/releases/tag') + "/v$version" } else { "$base/v$version/" }

function Step($m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }

function Find-SignTool {
  $c = Get-Command signtool.exe -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
  $hit = Get-ChildItem $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName -Descending | Select-Object -First 1
  if ($hit) { return $hit.FullName }
  throw "TUMR_SIGN_THUMBPRINT is set but signtool.exe was not found (install the Windows SDK)"
}

function Sign([string[]]$files) {
  if (-not $thumb) { return }
  $tool = Find-SignTool
  & $tool sign /sha1 $thumb /fd sha256 /tr $tsUrl /td sha256 /d "Toolchain Universal Mobile Runtime" @files
  if ($LASTEXITCODE -ne 0) { throw "signing failed" }
}

if (-not $SkipBuild) {
  Step "Third-party notices"
  & powershell -NoProfile -ExecutionPolicy Bypass -File scripts\third-party-notices.ps1
  if ($LASTEXITCODE -ne 0) { throw "notices failed" }

  Step "Release binaries"
  # cargo can reuse a cached build carrying another version string (seen after the version went
  # back to one built before): rebuild the shipped packages from scratch
  cargo clean --release -p tumr-core -p tumr-cli -p tumr-elevate -p tumr-desktop
  cargo build --release --locked -p tumr-core -p tumr-cli -p tumr-elevate
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
  Sign @("target\release\tumr.exe", "target\release\tumr-core.exe", "target\release\tumr-elevate.exe")

  Step "Desktop UI"
  npm --prefix apps\desktop\ui ci --no-audit --no-fund
  npm --prefix apps\desktop\ui run build
  if ($LASTEXITCODE -ne 0) { throw "UI build failed" }

  Step "Installer (NSIS, per user)"
  Push-Location apps\desktop
  try {
    # The release binaries are bundled through a separate config: the Tauri build script checks
    # every resource path, so listing the target release files in tauri.conf.json breaks every
    # debug build (clippy, tests) on a machine without a release build, such as CI.
    $cfg = @("--config", "tauri.release.conf.json")
    if ($thumb) {
      $json = '{"bundle":{"windows":{"certificateThumbprint":"' + $thumb + '","digestAlgorithm":"sha256","timestampUrl":"' + $tsUrl + '"}}}'
      $cfg += @("--config", $json)
    }
    node ui\node_modules\@tauri-apps\cli\tauri.js build --bundles nsis @cfg
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
  } finally { Pop-Location }
}

Step "Versions"
foreach ($f in @("tumr.exe", "tumr-core.exe")) {
  $v = (& "target\release\$f" --version 2>&1 | Out-String).Trim()
  Write-Host "$f -> $v"
  if ($v -notmatch [regex]::Escape(" $version")) { throw "$f reports '$v', expected $version" }
}

Step "Collect into $out"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Force $out | Out-Null
$nsis = Get-ChildItem "target\release\bundle\nsis" -Filter "*_$($version)_x64-setup.exe" | Select-Object -First 1
if (-not $nsis) { throw "installer for $version not found under target\release\bundle\nsis" }
$installer = Join-Path $out "Toolchain-Universal-Mobile-Runtime_$($version)_x64-setup.exe"
Copy-Item $nsis.FullName $installer

# portable ZIP: the same four programs; settings and toolchains go to %USERPROFILE%\.tumr as usual
$stage = Join-Path $out "zip-stage\Toolchain-Universal-Mobile-Runtime-$version"
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($f in @("tumr.exe", "tumr-core.exe", "tumr-desktop.exe", "tumr-elevate.exe")) { Copy-Item "target\release\$f" $stage }
Copy-Item THIRD_PARTY_NOTICES.md $stage
Copy-Item README.md $stage
$zip = Join-Path $out "Toolchain-Universal-Mobile-Runtime_$($version)_x64-portable.zip"
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
Remove-Item (Join-Path $out "zip-stage") -Recurse -Force

# CLI only (CI / headless machines): tumr + Core + elevation helper, no desktop app
$stage = Join-Path $out "cli-stage\tumr-cli-$version"
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($f in @("tumr.exe", "tumr-core.exe", "tumr-elevate.exe")) { Copy-Item "target\release\$f" $stage }
Copy-Item THIRD_PARTY_NOTICES.md $stage
$cli = Join-Path $out "tumr-cli_$($version)_x64.zip"
Compress-Archive -Path $stage -DestinationPath $cli -CompressionLevel Optimal
Remove-Item (Join-Path $out "cli-stage") -Recurse -Force

# VS Code extension (.vsix, Windows x64 target): the same file the Marketplace gets
$vsix = $null
if (-not $SkipVscode) {
  Step "VS Code extension"
  npm --prefix apps/vscode ci --no-audit --no-fund
  if ($LASTEXITCODE -ne 0) { throw "npm ci for the VS Code extension failed" }
  npm --prefix apps/vscode run package
  if ($LASTEXITCODE -ne 0) { throw "packaging the VS Code extension failed" }
  $extVersion = (Get-Content apps\vscode\package.json -Raw | ConvertFrom-Json).version
  $vsix = Join-Path $out "tumr-vscode-$($extVersion)-win32-x64.vsix"
  Copy-Item apps\vscode\dist\tumr-vscode.vsix $vsix
}

Step "Checks"
$files = @(Get-Item $installer, $zip, $cli)
if ($vsix) { $files += Get-Item $vsix }
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
if ($vsix) { $kinds[$vsix] = "vscode" }
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
  product  = "Toolchain Universal Mobile Runtime"
  releases = @([ordered]@{
      version           = $version
      channel           = $Channel
      date              = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd")
      min_windows_build = 22000
      signed            = $signed
      notes_url         = $notesUrl
      files             = @($entries)
    })
}
[System.IO.File]::WriteAllText((Join-Path $out "releases.json"), ($manifest | ConvertTo-Json -Depth 6), [System.Text.UTF8Encoding]::new($false))
Get-ChildItem $out | Format-Table Name, Length -AutoSize
if (-not $signed) { Write-Warning "UNSIGNED build: SmartScreen will warn. Set TUMR_SIGN_THUMBPRINT to sign (REL-001)." }
Write-Host "Release $version ready in $out"
