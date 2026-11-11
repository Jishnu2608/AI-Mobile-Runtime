# POC-003 sweep (throwaway): run bridge + Tauri viewer auto-measure at several resolutions.
param([string[]]$Sizes = @('720','1024','1280'))
$p = $PSScriptRoot
$env:ADB = 'D:\UMR-poc\android-sdk\platform-tools\adb.exe'
$env:SCRCPY_SERVER = 'D:\UMR-poc\downloads\scrcpy-server-v5.0.1'
$env:WEB_DIR = "$p\web"
foreach ($s in $Sizes) {
  Get-Process viewer, bridge -ErrorAction SilentlyContinue | Stop-Process -Force
  & $env:ADB shell pkill -f scrcpy 2>$null | Out-Null
  Start-Sleep 2
  $env:MAX_SIZE = $s
  $env:BITRATE = if ([int]$s -le 720) { '4000000' } else { '8000000' }
  $log = "$p\target\bridge-$s.log"
  $b = Start-Process "$p\target\release\bridge.exe" -WorkingDirectory "$p\target" -PassThru -WindowStyle Hidden -RedirectStandardOutput $log
  $t = Get-Date; do { Start-Sleep 1 } until ((Get-Content $log -ErrorAction SilentlyContinue) -match 'ready' -or ((Get-Date) - $t).TotalSeconds -gt 40)
  $q0 = (Get-Process qemu-system-x86_64-headless).CPU
  Start-Process "$p\target\release\viewer.exe" -ArgumentList '--auto' | Out-Null
  $t = Get-Date; do { Start-Sleep 2; $c = Get-Content $log } until (($c -match 'AUTO done') -or ((Get-Date) - $t).TotalSeconds -gt 180)
  $cores = [math]::Round(((Get-Process qemu-system-x86_64-headless).CPU - $q0) / ((Get-Date) - $t).TotalSeconds, 2)
  "=== max_size=$s"
  $c | Select-String 'session \d|\[result\] (LAT|FPS)' | ForEach-Object { $_.Line }
  "qemu cores avg: $cores"
  Get-Process viewer, bridge -ErrorAction SilentlyContinue | Stop-Process -Force
}
