# POC-013 sweep (throwaway): latency A/B for render path (2d vs gl) and input path (unary vs stream).
# Emulator must run with: -gpu host -grpc 8554 -grpc-use-token
param([string[]]$Variants = @('720x1600|2d|unary', '720x1600|gl|unary', '720x1600|2d|stream', '720x1600|gl|stream', '540x1200|gl|stream'),
if (-not $env:UMR_POC_ROOT) { throw 'Set UMR_POC_ROOT to the PoC managed root (the folder that holds android-sdk, downloads, android-user-home)' }
      [int]$Repeat = 1)
$p = $PSScriptRoot
$adb = "$env:UMR_POC_ROOT\android-sdk\platform-tools\adb.exe"
$ini = Get-ChildItem "$env:LOCALAPPDATA\Temp\avd\running" -Filter 'pid_*.ini' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$env:GRPC_TOKEN = ((Get-Content $ini.FullName | Select-String '^grpc.token=').Line -replace '^grpc.token=', '')
$env:GRPC = 'http://127.0.0.1:8554'
$env:WEB_DIR = "$p\web"
& $adb shell settings put system pointer_location 1 | Out-Null
& $adb shell input keyevent 3 | Out-Null
foreach ($rep in 1..$Repeat) { foreach ($v in $Variants) {
  $size, $render, $input = $v -split '\|'
  Get-Process viewer, grpcbridge -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 2
  $w, $h = $size -split 'x'; $env:OUT_W = $w; $env:OUT_H = $h
  $env:INPUT_MODE = $input; $env:VIEW_QUERY = "render=$render"
  $log = "$p\target\g013-$size-$render-$input-$rep.log"
  Start-Process "$p\target\release\grpcbridge.exe" -WorkingDirectory "$p\target" -WindowStyle Hidden -RedirectStandardOutput $log | Out-Null
  $t = Get-Date; do { Start-Sleep 1 } until ((Get-Content $log -ErrorAction SilentlyContinue) -match 'ready|error' -or ((Get-Date) - $t).TotalSeconds -gt 30)
  Start-Process "$p\target\release\viewer.exe" -ArgumentList '--auto' | Out-Null
  $t = Get-Date
  do { Start-Sleep 2; $c = Get-Content $log } until (($c -match 'AUTO done') -or ((Get-Date) - $t).TotalSeconds -gt 180)
  "=== $size render=$render input=$input run=$rep"
  $c | Select-String 'input error|stream ended|rpc n=|\[result\] (LAT|FPS|DRAW)' | ForEach-Object { $_.Line }
  Get-Process viewer, grpcbridge -ErrorAction SilentlyContinue | Stop-Process -Force
} }
& $adb shell settings put system pointer_location 0 | Out-Null
