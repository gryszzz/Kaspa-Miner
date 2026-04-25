$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Bin = Join-Path $Root "kaspa-miner.exe"
$DestDir = Join-Path $HOME "bin"
$Dest = Join-Path $DestDir "kaspa-miner.exe"

if (!(Test-Path $Bin)) {
    throw "kaspa-miner.exe was not found next to install-windows.ps1"
}

Unblock-File -Path $Bin -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
Copy-Item -Force $Bin $Dest
Unblock-File -Path $Dest -ErrorAction SilentlyContinue

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($UserPath -split ";") -notcontains $DestDir) {
    $NewPath = if ([string]::IsNullOrWhiteSpace($UserPath)) { $DestDir } else { "$UserPath;$DestDir" }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
}

Write-Host "KASPilot installed at $Dest"
Write-Host "Open a new terminal, then run: kaspa-miner --version"
& $Dest --version
