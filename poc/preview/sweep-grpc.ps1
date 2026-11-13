# POC-012 sweep (throwaway): emulator gRPC streamScreenshot -> grpcbridge -> Tauri viewer, several stream sizes.
# Emulator must run with: -gpu host -grpc 8554 -grpc-use-token
param([string[]]$Sizes = @('540x1200', '720x1600', '1080x2400'))
if (-not $env:UMR_POC_ROOT) { throw 'Set UMR_POC_ROOT to the PoC managed root (the folder that holds android-sdk, downloads, android-user-home)' }
$p = $PSScriptRoot
$adb = "$env:UMR_POC_ROOT\android-sdk\platform-tools\adb.exe"
$ini = Get-ChildItem "$env:LOCALAPPDATA\Temp\avd\running" -Filter 'pid_*.ini' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$env:GRPC_TOKEN = ((Get-Content $ini.FullName | Select-String '^grpc.token=').Line -replace '^grpc.token=', '')
$env:GRPC = 'http://127.0.0.1:8554'
$env:WEB_DIR = "$p\web"
& $adb shell settings put system pointer_location 1 | Out-Null
& $adb shell input keyevent 3 | Out-Null
foreach ($s in $Sizes) {
  Get-Process viewer, grpcbridge -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 2
  $w, $h = $s -split 'x'; $env:OUT_W = $w; $env:OUT_H = $h
  $log = "$p\target\grpcbridge-$s.log"
  Start-Process "$p\target\release\grpcbridge.exe" -WorkingDirectory "$p\target" -WindowStyle Hidden -RedirectStandardOutput $log | Out-Null
  $t = Get-Date; do { Start-Sleep 1 } until ((Get-Content $log -ErrorAction SilentlyContinue) -match 'ready|error' -or ((Get-Date) - $t).TotalSeconds -gt 30)
  $q0 = (Get-Process qemu-system-x86_64-headless).CPU
  Start-Process "$p\target\release\viewer.exe" -ArgumentList '--auto' | Out-Null
  $t = Get-Date; $wv = 0
  do { Start-Sleep 2; $c = Get-Content $log
       $b = Get-Process grpcbridge -ErrorAction SilentlyContinue
  } until (($c -match 'AUTO done') -or ((Get-Date) - $t).TotalSeconds -gt 180)
  $secs = ((Get-Date) - $t).TotalSeconds
  $cores = [math]::Round(((Get-Process qemu-system-x86_64-headless).CPU - $q0) / $secs, 2)
  $bcpu = if ($b) { [math]::Round($b.CPU / $secs, 2) } else { '?' }
  $bram = if ($b) { [int]($b.WS / 1MB) } else { '?' }
  "=== stream $s"
  $c | Select-String 'error|frames=|\[result\] (LAT|FPS)' | ForEach-Object { $_.Line }
  "qemu cores avg: $cores | grpcbridge cores: $bcpu, RAM $bram MB"
  Get-Process viewer, grpcbridge -ErrorAction SilentlyContinue | Stop-Process -Force
}
& $adb shell settings put system pointer_location 0 | Out-Null
