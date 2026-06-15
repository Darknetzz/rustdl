#requires -Version 5.1
<#
.SYNOPSIS
  Bump semver in Cargo.toml (patch by default). Does not edit CHANGELOG.

.DESCRIPTION
  After bumping, attempts to create an annotated tag rustdl-vX.Y.Z on HEAD when that
  commit already contains the new version. If Cargo.toml is not committed yet, run
  -TagOnly after your version-bump commit.

  Pushing dev to github publishes the stable release for the Cargo.toml version (via publish_stable_release).

.EXAMPLE
  .\scripts\bump_version.ps1
  .\scripts\bump_version.ps1 minor
  .\scripts\bump_version.ps1 -TagOnly
#>
[CmdletBinding()]
param(
    [ValidateSet('patch', 'minor', 'major')]
    [string] $Part = 'patch',
    [switch] $TagOnly,
    [switch] $NoTag
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$CargoToml = Join-Path $RepoRoot 'Cargo.toml'

function Get-CargoVersionFromFile {
    param([string] $Path = $CargoToml)
    $content = Get-Content -LiteralPath $Path -Raw
    if ($content -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"') {
        throw 'Could not parse version from Cargo.toml'
    }
    return $Matches[1]
}

function Get-CargoVersionAtHead {
    $head = git -C $RepoRoot show 'HEAD:Cargo.toml' 2>$null
    if (-not $head) {
        return $null
    }
    if ($head -match '(?m)^version = "(\d+\.\d+\.\d+)"') {
        return $Matches[1]
    }
    return $null
}

function New-RustdlVersionTag {
    param([string] $Version)

    $tag = "rustdl-v$Version"
    if (git -C $RepoRoot rev-parse -q --verify "refs/tags/$tag" 2>$null) {
        Write-Host "Tag $tag already exists at $(git -C $RepoRoot rev-parse --short $tag)."
        return
    }

    $headVer = Get-CargoVersionAtHead
    if ($headVer -ne $Version) {
        Write-Host "Cannot tag yet: HEAD has version '$headVer', expected '$Version'."
        Write-Host "Commit Cargo.toml with version $Version, then run: .\scripts\bump_version.ps1 -TagOnly"
        return
    }

    git -C $RepoRoot tag -a $tag -m "rustdl $Version"
    if ($LASTEXITCODE -ne 0) {
        throw "git tag failed for $tag"
    }

    Write-Host "Created annotated tag $tag at $(git -C $RepoRoot rev-parse --short HEAD)."
    Write-Host 'Push dev to github to publish the stable release (pre-push hook / push_dev), or run .\scripts\publish_stable_release.ps1.'
}

Push-Location $RepoRoot
try {
    if (-not (Test-Path -LiteralPath $CargoToml)) {
        throw "Cargo.toml not found at $CargoToml"
    }

    if ($TagOnly) {
        $version = Get-CargoVersionFromFile
        New-RustdlVersionTag $version
        exit 0
    }

    $current = Get-CargoVersionFromFile
    if ($current -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
        throw "Unexpected version format: $current"
    }
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    $patch = [int]$Matches[3]

    switch ($Part) {
        'patch' { $patch++ }
        'minor' {
            $minor++
            $patch = 0
        }
        'major' {
            $major++
            $minor = 0
            $patch = 0
        }
    }

    $new = "$major.$minor.$patch"
    $content = Get-Content -LiteralPath $CargoToml -Raw
    $content = [regex]::Replace($content, '(?m)^version = "\d+\.\d+\.\d+"', "version = `"$new`"", 1)
    [IO.File]::WriteAllText($CargoToml, $content)

    Write-Host "Bumped Cargo.toml version: $current -> $new"
    Write-Host 'Remember to add a [Unreleased] bullet in CHANGELOG.md for user-visible changes in this commit.'

    if (-not $NoTag) {
        New-RustdlVersionTag $new
    }
}
finally {
    Pop-Location
}
