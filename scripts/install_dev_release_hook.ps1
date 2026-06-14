#requires -Version 5.1
<#
.SYNOPSIS
  Enable automatic rustdl-dev publish after every successful push of dev to github.

  Sets core.hooksPath to .githooks (repo-local). Does not change shell aliases.

.EXAMPLE
  .\scripts\install_dev_release_hook.ps1
  .\scripts\install_dev_release_hook.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [switch] $Uninstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$HooksDir = Join-Path $RepoRoot '.githooks'
$PrePush = Join-Path $HooksDir 'pre-push'

Push-Location -LiteralPath $RepoRoot
try {
    if ($Uninstall) {
        $current = git config --local --get core.hooksPath 2>$null
        if ($current -eq '.githooks') {
            git config --local --unset core.hooksPath
            Write-Host 'Removed core.hooksPath (.githooks). Automatic dev release publish is disabled.'
        } else {
            Write-Host "core.hooksPath is '$current' (not .githooks); left unchanged."
        }
        exit 0
    }

    if (-not (Test-Path -LiteralPath $PrePush)) {
        throw "Missing hook: $PrePush"
    }

    git config --local core.hooksPath .githooks
    Write-Host 'Installed .githooks/pre-push (core.hooksPath = .githooks).'
    Write-Host ''
    Write-Host 'After every successful:  git push github dev'
    Write-Host '  → builds and refreshes https://github.com/Darknetzz/rustdl/releases/tag/rustdl-dev'
    Write-Host ''
    Write-Host 'Works with pushall and plain git push. Log: %TEMP%\rustdl-dev-release.log'
    Write-Host 'Requires: gh auth login, release build (runs automatically).'
    Write-Host ''
    Write-Host 'Disable: .\scripts\install_dev_release_hook.ps1 -Uninstall'
}
finally {
    Pop-Location
}
