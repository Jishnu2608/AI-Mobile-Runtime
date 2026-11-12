# Snapshot of everything the product must NOT change (TEST-001):
# user and machine PATH / JAVA_HOME / ANDROID_HOME / ANDROID_SDK_ROOT, and the known leak
# locations (file count + newest change time).
param([Parameter(Mandatory)] [string]$Out)

$ErrorActionPreference = 'Stop'
$vars = 'Path', 'JAVA_HOME', 'ANDROID_HOME', 'ANDROID_SDK_ROOT', 'ANDROID_USER_HOME'
$envSnap = [ordered]@{}
foreach ($scope in 'User', 'Machine') {
    foreach ($v in $vars) { $envSnap["$scope/$v"] = [Environment]::GetEnvironmentVariable($v, $scope) }
}

$leakDirs = @(
    "$env:USERPROFILE\.android",
    "$env:USERPROFILE\.expo",
    "$env:TEMP\netsimd",
    "$env:TEMP\avd",
    "$env:APPDATA\.flutter_settings",
    "$env:APPDATA\dart",
    "$env:LOCALAPPDATA\Pub\Cache"
)
$leaks = [ordered]@{}
foreach ($d in $leakDirs) {
    if (Test-Path $d) {
        $files = @(Get-ChildItem $d -Recurse -Force -File -ErrorAction SilentlyContinue)
        $newest = ($files | Measure-Object LastWriteTimeUtc -Maximum).Maximum
        $leaks[$d] = @{ exists = $true; files = $files.Count; newest = if ($newest) { $newest.ToString('o') } else { $null } }
    } else {
        $leaks[$d] = @{ exists = $false; files = 0; newest = $null }
    }
}

[ordered]@{ taken = (Get-Date).ToString('o'); env = $envSnap; leaks = $leaks } |
    ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 $Out
Write-Host "snapshot written to $Out"
