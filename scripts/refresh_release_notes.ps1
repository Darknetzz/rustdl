#requires -Version 5.1
<#
.SYNOPSIS
  Refresh GitHub stable release descriptions from CHANGELOG.md version sections.

.EXAMPLE
  .\scripts\refresh_release_notes.ps1 -DryRun
  .\scripts\refresh_release_notes.ps1
  .\scripts\refresh_release_notes.ps1 -Tag rustdl-v0.9.0
#>
[CmdletBinding()]
param(
    [switch] $DryRun,
    [string] $Repo = 'Darknetzz/rustdl',
    [string] $Tag = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$NotesFile = Join-Path $RepoRoot 'release-notes.md'
$ExtractScript = Join-Path $PSScriptRoot 'extract_release_notes.ps1'

Push-Location -LiteralPath $RepoRoot
try {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        throw 'gh CLI not found. Install GitHub CLI and run: gh auth login'
    }

    $tags = if ($Tag) {
        @($Tag)
    } else {
        gh release list --repo $Repo --limit 100 --json tagName -q '.[].tagName' |
            Where-Object { $_ -match '^rustdl-v\d+\.\d+\.\d+$' }
    }

    if (-not $tags) {
        Write-Host 'No rustdl-v* releases found.'
        exit 0
    }

    $updated = 0
    $skipped = 0

    foreach ($releaseTag in $tags) {
        try {
            & $ExtractScript -Tag $releaseTag -OutFile $NotesFile
            if ($LASTEXITCODE -ne 0) { throw "extract_release_notes failed (exit $LASTEXITCODE)" }

            if ($DryRun) {
                Write-Host "Would update $releaseTag"
                $updated++
                continue
            }

            gh release edit $releaseTag --repo $Repo --notes-file $NotesFile | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'gh release edit failed' }
            Write-Host "Updated $releaseTag"
            $updated++
        } catch {
            Write-Warning "Skipped $releaseTag : $($_.Exception.Message)"
            $skipped++
        }
    }

    Write-Host ''
    Write-Host "Done. Updated: $updated  Skipped: $skipped"
}
finally {
    Pop-Location
}
