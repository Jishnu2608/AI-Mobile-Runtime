# Local CI: runs the same checks as .github/workflows/ci.yml on this machine.
# Usage:  powershell -File scripts/ci-local.ps1 [-SkipRelease]
# Exit code 0 = all checks passed.
param([switch]$SkipRelease)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Make sure cargo is found even in shells opened before Rust was installed.
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + $env:Path
$env:RUSTFLAGS = '-D warnings'
$env:CARGO_TERM_COLOR = 'always'

$steps = @(
    @{ Name = 'Toolchain'; Cmd = { rustup show active-toolchain; rustc --version; cargo --version } },
    @{ Name = 'Format';    Cmd = { cargo fmt --all -- --check } },
    # The desktop app embeds the built web UI, so it must exist before Rust compiles it.
    @{ Name = 'UI build';  Cmd = { npm --prefix apps/desktop/ui ci --no-audit --no-fund; if ($LASTEXITCODE -eq 0) { npm --prefix apps/desktop/ui run build } } },
    @{ Name = 'Clippy';    Cmd = { cargo clippy --workspace --all-targets --locked } },
    @{ Name = 'Test';      Cmd = { cargo test --workspace --locked --no-fail-fast } }
)
if (-not $SkipRelease) {
    $steps += @{ Name = 'Release build'; Cmd = { cargo build --workspace --release --locked } }
    $steps += @{ Name = 'Smoke test';    Cmd = { & "$root\target\release\umr.exe" --version; & "$root\target\release\umr-core.exe" --version } }
}

$results = @()
foreach ($s in $steps) {
    Write-Host "`n=== $($s.Name) ===" -ForegroundColor Cyan
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $s.Cmd
    $ok = ($LASTEXITCODE -eq 0 -or $null -eq $LASTEXITCODE)
    $results += [pscustomobject]@{ Step = $s.Name; Result = $(if ($ok) { 'PASS' } else { 'FAIL' }); Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1) }
    if (-not $ok) { break }
    $global:LASTEXITCODE = 0
}

Write-Host "`n=== Summary ===" -ForegroundColor Cyan
$results | Format-Table -AutoSize | Out-String | Write-Host
if ($results.Result -contains 'FAIL' -or $results.Count -lt $steps.Count) { Write-Host 'LOCAL CI: FAILED' -ForegroundColor Red; exit 1 }
Write-Host 'LOCAL CI: PASSED' -ForegroundColor Green
exit 0
