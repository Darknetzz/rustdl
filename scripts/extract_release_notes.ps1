# Extract GitHub release notes from CHANGELOG.md for a rustdl tag.
# Prefers the dated ## [X.Y.Z] section matching the tag; falls back to ## [Unreleased].
#
# Usage: .\scripts\extract_release_notes.ps1 [-Changelog CHANGELOG.md] -Tag rustdl-v0.5.0 [-OutFile release-notes.md]
param(
    [string]$Changelog = "CHANGELOG.md",
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$OutFile = ""
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (-not (Test-Path -LiteralPath $Changelog)) {
    throw "CHANGELOG not found: $Changelog"
}

$ver = $Tag -replace '^rustdl-v', '' -replace '^v', ''

function Get-ChangelogSection {
    param(
        [string]$HeadingPattern
    )
    $lines = Get-Content -LiteralPath $Changelog
    $inSection = $false
    $section = New-Object System.Collections.Generic.List[string]

    foreach ($line in $lines) {
        $line = $line -replace '\r$', ''
        if ($line -match $HeadingPattern) {
            $inSection = $true
            $section.Add($line)
            continue
        }
        if ($inSection -and $line -match '^## \[') {
            break
        }
        if ($inSection) {
            $section.Add($line)
        }
    }

    if ($section.Count -eq 0) {
        return $null
    }
    return $section
}

function Test-SectionHasBody {
    param([System.Collections.Generic.List[string]]$Section)
    if ($Section.Count -le 1) {
        return $false
    }
    foreach ($line in $Section | Select-Object -Skip 1) {
        if ($line.Trim().Length -gt 0) {
            return $true
        }
    }
    return $false
}

$versionPattern = "^## \[$([regex]::Escape($ver))\]( - [0-9]{4}-[0-9]{2}-[0-9]{2})?$"
$notes = Get-ChangelogSection -HeadingPattern $versionPattern
if (-not ($notes -and (Test-SectionHasBody $notes))) {
    $notes = Get-ChangelogSection -HeadingPattern '^## \[Unreleased\]$'
}

if (-not ($notes -and (Test-SectionHasBody $notes))) {
    throw "No release notes found for $Tag in $Changelog"
}

$text = ($notes -join "`n") + "`n"
if ($OutFile) {
    Set-Content -LiteralPath $OutFile -Value $text -NoNewline -Encoding utf8
    Write-Host "Wrote $OutFile"
} else {
    Write-Output $text
}
