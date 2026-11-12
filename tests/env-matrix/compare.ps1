# Compares two collect.ps1 snapshots. Exit 0 = no global change and no leak; 1 = differences.
param([Parameter(Mandatory)] [string]$Before, [Parameter(Mandatory)] [string]$After)

$b = Get-Content $Before -Raw | ConvertFrom-Json
$a = Get-Content $After -Raw | ConvertFrom-Json
$problems = @()

foreach ($p in $b.env.PSObject.Properties) {
    $now = $a.env.($p.Name)
    if ($now -ne $p.Value) { $problems += "environment $($p.Name) changed: '$($p.Value)' -> '$now'" }
}
foreach ($p in $a.leaks.PSObject.Properties) {
    $was = $b.leaks.($p.Name)
    $is = $p.Value
    if ($is.exists -and -not $was.exists) { $problems += "leak: $($p.Name) was created ($($is.files) files)" }
    elseif ($is.files -gt $was.files) { $problems += "leak: $($p.Name) has $($is.files - $was.files) new files" }
    elseif ($is.newest -and $was.newest -and ([datetime]$is.newest -gt [datetime]$was.newest)) { $problems += "leak: files changed in $($p.Name)" }
}

if ($problems.Count -eq 0) { Write-Host 'PASS: no global environment change, no leaks' -ForegroundColor Green; exit 0 }
$problems | ForEach-Object { Write-Host "FAIL: $_" -ForegroundColor Red }
exit 1
