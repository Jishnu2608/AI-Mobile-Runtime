# POC-006 (throwaway): drive `flutter run --machine` over its JSON protocol and time hot reload / hot restart.
# Expects env from the POC recipe (JAVA_HOME, ANDROID_HOME, PUB_CACHE, GRADLE_USER_HOME, JAVA_TOOL_OPTIONS, PATH with flutter).
# The app prints "UMR_RENDER <label>" from a build() method; machine mode reports prints as app.log events.
param(
  [Parameter(Mandatory)] [string]$Project,
  [Parameter(Mandatory)] [string]$MarkerFile,
  [string]$Device = 'emulator-5554',
  [int]$Reloads = 3,
  [int]$Restarts = 2
)
$psi = New-Object Diagnostics.ProcessStartInfo 'cmd.exe', "/c flutter run --machine -d $Device"
$psi.WorkingDirectory = $Project
$psi.RedirectStandardOutput = $true; $psi.RedirectStandardInput = $true; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = [Diagnostics.Process]::Start($psi)
$appId = $null; $events = 0

# Keep one pending read; a timed-out ReadLineAsync must not be replaced by a second one.
$script:pending = $null
function NextLine([int]$ms) {
  if (-not $script:pending) { $script:pending = $p.StandardOutput.ReadLineAsync() }
  if ($script:pending.Wait($ms)) { $l = $script:pending.Result; $script:pending = $null; return $l }
  return $null
}
function WaitFor([scriptblock]$pred, [int]$timeoutSec) {
  $w = [Diagnostics.Stopwatch]::StartNew()
  while ($w.Elapsed.TotalSeconds -lt $timeoutSec) {
    $l = NextLine 1000
    if ($l) { $script:events++; if ($l -match '"event":"app.progress".*"finished":false' ) { } ; if (& $pred $l) { return $l } }
  }
  return $null
}

$started = WaitFor { param($l) $l -match '"event":"app.started"' } 900
$startLine = $null
if (-not $started) { "FLUTTER app.started not seen within 900 s"; $p.Kill(); exit 1 }
"FLUTTER cold run (build + install + start) {0:N1} s" -f $sw.Elapsed.TotalSeconds
$appId = ([regex]::Match($started, '"appId":"([^"]+)"')).Groups[1].Value

$id = 100
function Cycle([string]$from, [string]$to, [bool]$full) {
  $text = [IO.File]::ReadAllText($MarkerFile)
  [IO.File]::WriteAllText($MarkerFile, $text.Replace("UMR_RENDER $from", "UMR_RENDER $to"))
  $script:id++
  $req = '[{"id":' + $script:id + ',"method":"app.restart","params":{"appId":"' + $appId + '","fullRestart":' + ($full.ToString().ToLower()) + ',"pause":false,"reason":"manual"}}]'
  $w = [Diagnostics.Stopwatch]::StartNew()
  $p.StandardInput.WriteLine($req); $p.StandardInput.Flush()
  $resp = $null; $logT = $null; $respT = $null
  while ($w.Elapsed.TotalSeconds -lt 120 -and (-not $resp -or -not $logT)) {
    $l = NextLine 1000
    if (-not $l) { continue }
    if (-not $resp -and $l -match ('"id":' + $script:id + '[,}]')) { $resp = $l; $respT = $w.Elapsed.TotalMilliseconds }
    if (-not $logT -and $l -match "UMR_RENDER $to") { $logT = $w.Elapsed.TotalMilliseconds }
  }
  $kind = if ($full) { 'hot restart' } else { 'hot reload' }
  "{0} {1}->{2}: response {3:N0} ms, re-render seen {4:N0} ms  {5}" -f $kind, $from, $to, $respT, $logT, ($resp -replace '.*"result":', 'result=')
}
$n = 0
foreach ($i in 1..$Reloads)  { Cycle "f$n" "f$($n+1)" $false; $n++; Start-Sleep 1 }
foreach ($i in 1..$Restarts) { Cycle "f$n" "f$($n+1)" $true;  $n++; Start-Sleep 1 }
# restore marker label and stop
$text = [IO.File]::ReadAllText($MarkerFile); [IO.File]::WriteAllText($MarkerFile, $text.Replace("UMR_RENDER f$n", 'UMR_RENDER f0'))
$p.StandardInput.WriteLine('[{"id":999,"method":"app.stop","params":{"appId":"' + $appId + '"}}]'); $p.StandardInput.Flush()
Start-Sleep 3; if (-not $p.HasExited) { $p.Kill() }
"FLUTTER machine events read: $events"
