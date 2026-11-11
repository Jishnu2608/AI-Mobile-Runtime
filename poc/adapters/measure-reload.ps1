# POC-006 (throwaway): time from a source-file edit to the app re-rendering, using a log marker.
# The app logs "UMR_RENDER <label>" when it renders; we replace the label and wait for the new line in logcat.
# (adb screencap / emulator console screenshots are black under -gpu host, so screen hashing is not usable.)
param(
  [Parameter(Mandatory)] [string]$File,
  [Parameter(Mandatory)] [string]$From,     # current label
  [Parameter(Mandatory)] [string]$To,       # new label
  [string]$Adb = $env:ADB,
  [string]$LogFilter = '*:I',
  [int]$TimeoutSec = 120,
  [scriptblock]$Trigger = $null            # optional action after saving (e.g. Flutter hot-reload command)
)
& $Adb logcat -c
$psi = New-Object Diagnostics.ProcessStartInfo $Adb, "logcat -v epoch $LogFilter"
$psi.RedirectStandardOutput = $true; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
$p = [Diagnostics.Process]::Start($psi)
Start-Sleep -Milliseconds 300
$text = [IO.File]::ReadAllText($File)
if (-not $text.Contains("UMR_RENDER $From")) { $p.Kill(); throw "marker 'UMR_RENDER $From' not found in $File" }
$sw = [Diagnostics.Stopwatch]::StartNew()
[IO.File]::WriteAllText($File, $text.Replace("UMR_RENDER $From", "UMR_RENDER $To"))
if ($Trigger) { & $Trigger }
$found = $false; $task = $null
while (-not $found -and $sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
  if (-not $task) { $task = $p.StandardOutput.ReadLineAsync() }   # keep one pending read
  if ($task.Wait(1000)) { $l = $task.Result; $task = $null; if ($l -and $l.Contains("UMR_RENDER $To")) { $found = $true } }
}
$sw.Stop(); $p.Kill()
if ($found) { "RELOAD $From->$To rendered after {0:N0} ms" -f $sw.Elapsed.TotalMilliseconds } else { "RELOAD $From->$To timeout" }
