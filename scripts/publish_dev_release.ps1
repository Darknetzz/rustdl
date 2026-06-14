#requires -Version 5.1
<#
.SYNOPSIS
  Build and publish a rolling pre-release on GitHub (tag rustdl-dev).

  Updates the same GitHub release on every run — no GitHub Actions required.
  Requires: gh CLI (authenticated), release binary build (or -SkipBuild).

.EXAMPLE
  .\scripts\publish_dev_release.ps1
  .\scripts\publish_dev_release.ps1 -DryRun
  .\scripts\publish_dev_release.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [switch] $DryRun,
    [switch] $SkipBuild,
    [string] $Repo = 'Darknetzz/rustdl',
    [string] $Tag = 'rustdl-dev',
    [string] $Commit = '',
    [switch] $AllowDirty
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$CargoToml = Join-Path $RepoRoot 'Cargo.toml'
$ChangelogPath = Join-Path $RepoRoot 'CHANGELOG.md'
$NotesFile = Join-Path $RepoRoot 'dev-release-notes.md'

function Get-CargoVersion {
    $content = Get-Content -LiteralPath $CargoToml -Raw
    if ($content -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"') {
        throw 'Could not parse version from Cargo.toml'
    }
    return $Matches[1]
}

function Get-UnreleasedChangelogBody {
    if (-not (Test-Path -LiteralPath $ChangelogPath)) {
        return ''
    }
    $lines = Get-Content -LiteralPath $ChangelogPath
    $inUnreleased = $false
    $body = New-Object System.Collections.Generic.List[string]
    foreach ($line in $lines) {
        if ($line -eq '## [Unreleased]') {
            $inUnreleased = $true
            continue
        }
        if ($inUnreleased -and $line -match '^## \[') {
            break
        }
        if ($inUnreleased) {
            $body.Add($line)
        }
    }
    return ($body -join "`n").Trim()
}

function Write-DevReleaseNotes {
    param(
        [string] $Version,
        [string] $FullCommit,
        [string] $ShortCommit,
        [string] $Branch,
        [string] $OutPath
    )

    $built = Get-Date -Format 'yyyy-MM-dd HH:mm K'
    $unreleased = Get-UnreleasedChangelogBody
    $compare = "https://github.com/$Repo/compare/rustdl-dev...$ShortCommit"

    $text = @"
# rustdl dev (rolling)

Pre-release build from the tip of ``$Branch``. Stable builds: [GitHub Releases](https://github.com/$Repo/releases).

| | |
| --- | --- |
| **Commit** | [$ShortCommit](https://github.com/$Repo/commit/$FullCommit) |
| **Cargo version** | $Version |
| **Built** | $built |

"@

    if ($unreleased) {
        $text += @"

## [Unreleased] (from CHANGELOG)

$unreleased
"@
    } else {
        $text += @"

## [Unreleased] (from CHANGELOG)

_No bullets under `[Unreleased]` yet._
"@
    }

    $text += @"


---
Compare to previous dev build: $compare
"@

    [IO.File]::WriteAllText($OutPath, $text)
}

function Get-ReleaseBinaryPath {
    $win = Join-Path $RepoRoot 'target\release\rustdl.exe'
    if (Test-Path -LiteralPath $win) {
        return $win
    }
    $unix = Join-Path $RepoRoot 'target/release/rustdl'
    if (Test-Path -LiteralPath $unix) {
        return $unix
    }
    throw 'Release binary not found. Run .\scripts\build_binary.ps1 first (or drop -SkipBuild).'
}

Push-Location -LiteralPath $RepoRoot
try {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        throw 'gh CLI not found. Install GitHub CLI and run: gh auth login'
    }

    $null = gh auth status 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw 'gh is not authenticated. Run: gh auth login'
    }

    if (-not $Commit) {
        $Commit = (git rev-parse HEAD).Trim()
    }
    $shortCommit = (git rev-parse --short $Commit).Trim()
    $branch = (git rev-parse --abbrev-ref HEAD).Trim()
    $version = Get-CargoVersion
    $title = "rustdl dev (rolling)"

    if ((git status --porcelain) -and -not $AllowDirty) {
        throw 'Working tree is not clean. Commit/stash or pass -AllowDirty.'
    }

    Write-Host 'Rolling dev release'
    Write-Host "  Repo:    $Repo"
    Write-Host "  Tag:     $Tag"
    Write-Host "  Commit:  $shortCommit ($Commit)"
    Write-Host "  Version: $version"

    if (-not $SkipBuild -and -not $DryRun) {
        Write-Host ''
        Write-Host 'Building release binary...'
        & (Join-Path $PSScriptRoot 'build_binary.ps1')
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } elseif (-not $SkipBuild -and $DryRun) {
        Write-Host ''
        Write-Host 'Would run: .\scripts\build_binary.ps1'
    }

    Write-DevReleaseNotes -Version $version -FullCommit $Commit -ShortCommit $shortCommit -Branch $branch -OutPath $NotesFile

    if ($DryRun) {
        $binaryHint = Join-Path $RepoRoot 'target\release\rustdl.exe'
        if (-not (Test-Path -LiteralPath $binaryHint)) {
            $binaryHint = Join-Path $RepoRoot 'target/release/rustdl'
        }
        Write-Host ''
        Write-Host "Dry run: would publish $Tag to $Repo"
        Write-Host "  Binary:  $binaryHint"
        Write-Host "  Notes:   $NotesFile"
        exit 0
    }

    $binary = Get-ReleaseBinaryPath

    $releaseExists = $false
    gh release view $Tag --repo $Repo 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) {
        $releaseExists = $true
    }

    if ($releaseExists) {
        Write-Host ''
        Write-Host "Updating existing release $Tag..."
        gh release edit $Tag `
            --repo $Repo `
            --title $title `
            --notes-file $NotesFile `
            --prerelease `
            --target $Commit
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

        gh release upload $Tag $binary --repo $Repo --clobber
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } else {
        Write-Host ''
        Write-Host "Creating release $Tag..."
        gh release create $Tag `
            --repo $Repo `
            --title $title `
            --notes-file $NotesFile `
            --prerelease `
            --target $Commit `
            $binary
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    $url = "https://github.com/$Repo/releases/tag/$Tag"
    Write-Host ''
    Write-Host "Published rolling dev release: $url"
}
finally {
    Pop-Location
}
