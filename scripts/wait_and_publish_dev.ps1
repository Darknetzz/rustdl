#requires -Version 5.1
<#
.SYNOPSIS
  Wait until github/dev matches Commit, then publish the rolling rustdl-dev release.

  Invoked in the background by .githooks/pre-push after git push github dev.
#>
[CmdletBinding()]
param(
    [string] $Remote = 'github',
    [Parameter(Mandatory = $true)]
    [string] $Commit,
    [int] $TimeoutSec = 180
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$LogFile = Join-Path $env:TEMP 'rustdl-dev-release.log'

function Write-Log {
    param([string] $Message)
    $line = "{0} {1}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
    Add-Content -LiteralPath $LogFile -Value $line
}

Push-Location -LiteralPath $RepoRoot
try {
    Write-Log "Waiting for $Remote dev to reach $Commit ..."
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $matched = $false

    while ((Get-Date) -lt $deadline) {
        $remoteSha = (
            git ls-remote $Remote refs/heads/dev 2>$null |
            ForEach-Object { ($_ -split '\s+')[0] } |
            Select-Object -First 1
        )
        if ($remoteSha -eq $Commit) {
            $matched = $true
            break
        }
        Start-Sleep -Seconds 2
    }

    if (-not $matched) {
        Write-Log "Timed out after ${TimeoutSec}s (remote dev never reached $Commit)."
        exit 1
    }

    Write-Log "Remote matched; publishing rustdl-dev ..."
    & (Join-Path $PSScriptRoot 'publish_dev_release.ps1') -Commit $Commit
    if ($LASTEXITCODE -ne 0) {
        Write-Log "publish_dev_release.ps1 failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    Write-Log "Published rustdl-dev for $Commit"

    Write-Log "Publishing stable release (if Cargo version is new) ..."
    & (Join-Path $PSScriptRoot 'publish_stable_release.ps1') -Commit $Commit -SkipBuild
    if ($LASTEXITCODE -ne 0) {
        Write-Log "publish_stable_release.ps1 failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    Write-Log "Stable release step finished for $Commit"
    Write-Host "Dev release publish finished (log: $LogFile)"
}
catch {
    Write-Log "Error: $($_.Exception.Message)"
    exit 1
}
finally {
    Pop-Location
}
