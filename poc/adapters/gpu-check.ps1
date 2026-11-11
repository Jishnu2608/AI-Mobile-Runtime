# POC-006 (throwaway): boot the emulator with a given GPU mode, open Settings, take a host-side gRPC screenshot,
# and report whether app content is rendered (fraction of non-black pixels).
param([string]$Gpu = 'host', [switch]$Cold)
$r = 'D:\UMR-poc'; $adb = "$r\android-sdk\platform-tools\adb.exe"
$env:ANDROID_HOME = "$r\android-sdk"; $env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
$env:ANDROID_USER_HOME = "$r\android-user-home"; $env:ANDROID_AVD_HOME = "$r\android-user-home\avd"
& $adb emu kill 2>$null | Out-Null; Start-Sleep 8
$args2 = @('-avd', 'medium_phone', '-no-window', '-no-audio', '-no-boot-anim', '-gpu', $Gpu, '-grpc', '8554', '-grpc-use-token')
if ($Cold) { $args2 += '-no-snapshot-load' }
Start-Process "$r\android-sdk\emulator\emulator.exe" -ArgumentList $args2 -WindowStyle Hidden -RedirectStandardOutput "$r\logs\emu-gpucheck-$Gpu.log" | Out-Null
$t = Get-Date; & $adb wait-for-device 2>$null
do { Start-Sleep 1 } until ((& $adb shell getprop sys.boot_completed 2>$null) -match '1' -or ((Get-Date) - $t).TotalSeconds -gt 300)
"gpu=$Gpu cold=$Cold boot s=" + [int]((Get-Date) - $t).TotalSeconds
Select-String "$r\logs\emu-gpucheck-$Gpu.log" -Pattern 'gles_mode_selected' | ForEach-Object { $_.Line.Trim() }
& $adb shell input keyevent KEYCODE_WAKEUP; & $adb shell am start -W -a android.settings.SETTINGS | Out-Null; Start-Sleep 4
$ini = Get-ChildItem "$env:LOCALAPPDATA\Temp\avd\running" -Filter 'pid_*.ini' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$env:GRPC_TOKEN = ((Get-Content $ini.FullName | Select-String '^grpc.token=').Line -replace '^grpc.token=', '')
$env:WEB_DIR = 'x'; $env:SHOT = "$r\logs\gpucheck-$Gpu.png"
& "$PSScriptRoot\..\preview\target\release\grpcbridge.exe" | Select-Object -Last 1
Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Bitmap]::FromFile($env:SHOT); $nb = 0; $tot = 0
for ($y = 100; $y -lt $img.Height - 200; $y += 20) { for ($x = 0; $x -lt $img.Width; $x += 20) { $c = $img.GetPixel($x, $y); $tot++; if (($c.R + $c.G + $c.B) -gt 60) { $nb++ } } }
$img.Dispose()
"non-black pixel fraction (app area): {0:P1}" -f ($nb / $tot)
