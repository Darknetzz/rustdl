#requires -Version 5.1
<#
.SYNOPSIS
  Push dev to GitHub, publish the rolling dev release, then mirror to GitLab.

  Use this instead of plain `git push` when you want every dev push to refresh
  the rustdl-dev GitHub pre-release (no GitHub Actions).

.EXAMPLE
  .\scripts\push_dev.ps1
  .\scripts\push_dev.ps1 -DryRun
  .\scripts\push_dev.ps1 -SkipGitlab
  .\scripts\push_dev.ps1 -SkipPublish
#>
[CmdletBinding()]
param(
    [switch] $DryRun,
    [switch] $SkipPublish,
    [switch] $SkipGitlab,
    [string] $GithubRemote = 'github',
    [string] $GitlabRemote = 'gitlab'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $RepoRoot
try {
    $branch = (git rev-parse --abbrev-ref HEAD).Trim()
    if ($branch -ne 'dev') {
        Write-Warning "Not on dev branch (on $branch)."
    }

    Write-Host "--- $GithubRemote ---"
    if ($DryRun) {
        Write-Host "Would run: git push $GithubRemote dev"
    } else {
        git push $GithubRemote dev
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    if (-not $SkipPublish) {
        Write-Host ''
        Write-Host '--- rolling dev release ---'
        $publishArgs = @()
        if ($DryRun) { $publishArgs += '-DryRun' }
        & (Join-Path $PSScriptRoot 'publish_dev_release.ps1') @publishArgs
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

        Write-Host ''
        Write-Host '--- stable release (if new version) ---'
        $stableArgs = @('-SkipBuild')
        if ($DryRun) { $stableArgs += '-DryRun' }
        & (Join-Path $PSScriptRoot 'publish_stable_release.ps1') @stableArgs
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    if (-not $SkipGitlab) {
        Write-Host ''
        Write-Host "--- $GitlabRemote ---"
        if ($DryRun) {
            Write-Host "Would run: git push $GitlabRemote dev"
        } else {
            git push $GitlabRemote dev
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "GitLab push failed (GitHub and dev release may already be updated)."
                exit $LASTEXITCODE
            }
        }
    }

    if ($DryRun) {
        Write-Host ''
        Write-Host 'Dry run complete.'
    } else {
        Write-Host ''
        Write-Host 'Push and publish complete.'
    }
}
finally {
    Pop-Location
}
