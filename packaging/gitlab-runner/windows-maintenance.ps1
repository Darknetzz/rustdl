# Weekly hygiene for the rustdl Windows GitLab runner (wingl-runner01).
# Installed as scheduled task rustdl-runner-maintenance (SYSTEM, Sunday 04:00).
#Requires -RunAsAdministrator
$ErrorActionPreference = "Continue"
$logDir = "C:\GitLab-Runner\logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$log = Join-Path $logDir "maintenance.log"

function Write-Log([string]$Message) {
    $line = "{0} {1}" -f (Get-Date).ToString("o"), $Message
    Add-Content -Path $log -Value $line
    Write-Host $line
}

Write-Log "maintenance start"

Write-Log "rustup update"
& rustup update stable
if ($LASTEXITCODE -ne 0) { Write-Log "rustup update exit $LASTEXITCODE" }

Write-Log "choco upgrade"
& choco upgrade git powershell-core pwsh cmake ninja 7zip -y --no-progress
if ($LASTEXITCODE -ne 0) { Write-Log "choco upgrade exit $LASTEXITCODE" }

$compilers = Get-Process -Name rustc, cargo, sccache -ErrorAction SilentlyContinue
if ($compilers) {
    Write-Log "skip build prune; compilers running"
} else {
    $cutoff = (Get-Date).AddDays(-14)
    $targets = Get-ChildItem -Path "C:\GitLab-Runner\builds" -Recurse -Directory -Filter target -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -lt $cutoff }
    foreach ($dir in $targets) {
        Write-Log "prune $($dir.FullName)"
        Remove-Item -LiteralPath $dir.FullName -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Log "pruned $($targets.Count) target dir(s)"
}

Write-Log "windows update search"
$reboot = $false
try {
    $session = New-Object -ComObject Microsoft.Update.Session
    $searcher = $session.CreateUpdateSearcher()
    $found = $searcher.Search("IsInstalled=0 and Type='Software' and IsHidden=0")
    Write-Log "pending updates: $($found.Updates.Count)"
    if ($found.Updates.Count -gt 0) {
        $coll = New-Object -ComObject Microsoft.Update.UpdateColl
        foreach ($update in $found.Updates) {
            if (-not $update.EulaAccepted) { $update.AcceptEula() | Out-Null }
            Write-Log "queue $($update.Title)"
            [void]$coll.Add($update)
        }
        $downloader = $session.CreateUpdateDownloader()
        $downloader.Updates = $coll
        $downloaded = $downloader.Download()
        Write-Log "download result $($downloaded.ResultCode)"
        $installer = $session.CreateUpdateInstaller()
        $installer.Updates = $coll
        $installer.AllowSourcePrompts = $false
        $installer.IsForced = $true
        if ($installer.PSObject.Properties.Name -contains "ForceQuiet") { $installer.ForceQuiet = $true }
        $installed = $installer.Install()
        Write-Log "install result $($installed.ResultCode) reboot=$($installed.RebootRequired)"
        $reboot = [bool]$installed.RebootRequired
    }
} catch {
    Write-Log "windows update failed: $($_.Exception.Message)"
}

Write-Log "maintenance done reboot=$reboot"
if ($reboot) {
    Write-Log "rebooting"
    Restart-Computer -Force
}
