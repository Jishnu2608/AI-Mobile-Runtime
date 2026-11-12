# Prepares the VM state for experiments that need one. Run inside the throwaway VM only.
param(
    [Parameter(Mandatory)] [ValidateSet(6, 8)] [int]$Experiment,
    [string]$Proxy
)
$ErrorActionPreference = 'Stop'

switch ($Experiment) {
    6 {
        # Conflicting JDKs: a JDK 8 first on the user PATH and a JAVA_HOME pointing nowhere.
        $jdk8 = 'C:\jdk8'
        if (-not (Test-Path "$jdk8\bin\java.exe")) {
            Write-Host "Put an unpacked JDK 8 into $jdk8 first (e.g. Temurin 8 zip)." -ForegroundColor Yellow
            exit 1
        }
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        [Environment]::SetEnvironmentVariable('Path', "$jdk8\bin;$userPath", 'User')
        [Environment]::SetEnvironmentVariable('JAVA_HOME', 'C:\Program Files\Java\jdk-removed', 'User')
        Write-Host 'JDK 8 first on user PATH, stale JAVA_HOME set. Open a NEW terminal, then run collect.ps1.'
    }
    8 {
        if (-not $Proxy) { Write-Host 'Pass -Proxy http://host:port' -ForegroundColor Yellow; exit 1 }
        [Environment]::SetEnvironmentVariable('HTTPS_PROXY', $Proxy, 'User')
        [Environment]::SetEnvironmentVariable('HTTP_PROXY', $Proxy, 'User')
        Write-Host "Proxy set for the user. Block direct internet in the VM firewall to prove all traffic uses $Proxy."
    }
}
