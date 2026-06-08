#requires -Version 5.1
<#
.SYNOPSIS
  Bump semver in Cargo.toml (patch by default). Does not edit CHANGELOG.

.EXAMPLE
  .\scripts\bump_version.ps1
  .\scripts\bump_version.ps1 minor
#>
[CmdletBinding()]
param(
    [ValidateSet('patch', 'minor', 'major')]
    [string] $Part = 'patch'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$CargoToml = Join-Path $RepoRoot 'Cargo.toml'

if (-not (Test-Path -LiteralPath $CargoToml)) {
    throw "Cargo.toml not found at $CargoToml"
}

$content = Get-Content -LiteralPath $CargoToml -Raw
if ($content -notmatch '(?m)^version = "(\d+)\.(\d+)\.(\d+)"') {
    throw 'Could not parse version from Cargo.toml'
}

$major = [int]$Matches[1]
$minor = [int]$Matches[2]
$patch = [int]$Matches[3]
$current = "$major.$minor.$patch"

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
$content = [regex]::Replace($content, '(?m)^version = "\d+\.\d+\.\d+"', "version = `"$new`"", 1)
[IO.File]::WriteAllText($CargoToml, $content)

Write-Host "Bumped Cargo.toml version: $current -> $new"
Write-Host 'Remember to add a [Unreleased] bullet in CHANGELOG.md for user-visible changes in this commit.'
