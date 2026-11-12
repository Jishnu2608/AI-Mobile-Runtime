# TEST-001 diagnosis: which environment-scan probe creates %USERPROFILE%\.android?
#
# Runs every tool the product's environment scan starts, one at a time, with the product's
# isolated environment (like umr-env `child_env`). Before each probe an EMPTY .android folder is
# removed (only on a throwaway CI runner); after the probe the script reports whether it came back.
# Each probe runs twice: (a) isolated environment as the product uses it today, and (b) the same
# plus USERPROFILE / HOME / HOMEDRIVE / HOMEPATH and Java user.home pointed at a private folder,
# to see whether redirecting the profile is a working fix.
#
# Usage (CI only): ./tests/env-matrix/leak-probe.ps1 -Toolchains D:\umr-toolchains
param([Parameter(Mandatory = $true)][string]$Toolchains)
$ErrorActionPreference = 'Continue'
$leak = Join-Path $env:USERPROFILE '.android'
$sdk = Join-Path $Toolchains 'android-sdk'
$userHome = Join-Path $Toolchains 'android-user-home'
$tmp = Join-Path $Toolchains 'tmp'
$fakeProfile = Join-Path $Toolchains 'probe-profile'
New-Item -ItemType Directory -Force $tmp, $userHome, $fakeProfile | Out-Null

$probes = @(
    @{ Name = 'adb version';             Exe = "$sdk\platform-tools\adb.exe";                Args = 'version' },
    @{ Name = 'emulator -version';       Exe = "$sdk\emulator\emulator.exe";                 Args = '-version' },
    @{ Name = 'emulator -accel-check';   Exe = "$sdk\emulator\emulator.exe";                 Args = '-accel-check' },
    @{ Name = 'android -V --no-metrics'; Exe = "$sdk\cmdline-tools\latest\bin\android.exe";  Args = '-V --no-metrics' }
)
$java = Get-ChildItem (Join-Path $Toolchains 'jdks') -Directory -ErrorAction SilentlyContinue | Select-Object -First 1
if ($java) { $probes += @{ Name = 'java -version'; Exe = "$($java.FullName)\bin\java.exe"; Args = '-version' } }
# Tools the scan looks up on PATH (umr-env tools.rs).
foreach ($t in 'node', 'npm', 'pnpm', 'yarn', 'bun', 'git', 'flutter', 'android') {
    $cmd = Get-Command $t -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) {
        $a = if ($t -eq 'android') { '-V --no-metrics' } else { '--version' }
        $probes += @{ Name = "PATH $t $a"; Exe = $cmd.Source; Args = $a }
    }
}

function Clear-Leak {
    if (Test-Path $leak) {
        if (@(Get-ChildItem -Force $leak).Count -eq 0) { Remove-Item -Force $leak }
        else { Write-Host "  (.android is not empty; left in place: $((Get-ChildItem -Force $leak).Name -join ', '))" }
    }
}

function Invoke-Probe($p, [bool]$redirect) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo $p.Exe
    $psi.Arguments = $p.Args
    $psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    $e = $psi.Environment
    foreach ($v in 'JAVA_HOME', 'ANDROID_HOME', 'ANDROID_SDK_ROOT') { $e.Remove($v) | Out-Null }
    $e['ANDROID_HOME'] = $sdk; $e['ANDROID_SDK_ROOT'] = $sdk
    $e['ANDROID_USER_HOME'] = $userHome; $e['ANDROID_EMULATOR_HOME'] = $userHome
    $e['ANDROID_AVD_HOME'] = Join-Path $userHome 'avd'
    $e['TEMP'] = $tmp; $e['TMP'] = $tmp
    $e['ANDROID_ADB_SERVER_PORT'] = '5137'
    $jto = "-Djdk.net.unixdomain.tmpdir=$tmp"
    if ($redirect) {
        $e['USERPROFILE'] = $fakeProfile; $e['HOME'] = $fakeProfile
        $e['HOMEDRIVE'] = Split-Path $fakeProfile -Qualifier; $e['HOMEPATH'] = $fakeProfile.Substring(2)
        $jto += " -Duser.home=$fakeProfile"
    }
    $e['JAVA_TOOL_OPTIONS'] = $jto
    $proc = [System.Diagnostics.Process]::Start($psi)
    $null = $proc.StandardOutput.ReadToEnd(); $null = $proc.StandardError.ReadToEnd()
    $null = $proc.WaitForExit(120000)
    return $proc.ExitCode
}

if ((Test-Path $leak) -and @(Get-ChildItem -Force $leak).Count -gt 0) {
    Write-Host "CANNOT DIAGNOSE: $leak already has files ($((Get-ChildItem -Force $leak).Name -join ', ')); run this on a clean runner."
    exit 0
}
$found = @()
foreach ($p in $probes) {
    if (-not (Test-Path $p.Exe)) { Write-Host ("{0,-26} not installed" -f $p.Name); continue }
    foreach ($redirect in $false, $true) {
        Clear-Leak
        $code = Invoke-Probe $p $redirect
        $made = Test-Path $leak
        $how = if ($redirect) { 'profile redirected' } else { 'isolated env      ' }
        Write-Host ("{0,-26} {1}  exit {2,-4}  creates .android: {3}" -f $p.Name, $how, $code, $made)
        if ($made) { $found += "$($p.Name) ($how)" }
    }
}
# adb also starts a server for some commands; stop ours so it does not linger.
& "$sdk\platform-tools\adb.exe" -P 5137 kill-server 2>$null | Out-Null
if ($found) { Write-Host "LEAK SOURCE: $($found -join '; ')" } else { Write-Host 'LEAK SOURCE: none of the scan probes' }
