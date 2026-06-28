# Extract GitHub release notes from CHANGELOG.md for a rustdl tag.
# Uses the ## [X.Y.Z] section matching the tag (date suffix optional).
# Does not use ## [Unreleased] — finalize CHANGELOG before publishing stable releases.
#
# Usage: .\scripts\extract_release_notes.ps1 [-Changelog CHANGELOG.md] -Tag rustdl-v0.5.0 [-OutFile release-notes.md]
param(
    [string]$Changelog = "CHANGELOG.md",
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$OutFile = "",
    [switch]$IncludeHeading
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

function Format-ReleaseNotesBody {
    param([System.Collections.Generic.List[string]]$Section)

    if ($IncludeHeading) {
        return ($Section -join "`n").TrimEnd() + "`n"
    }

    $start = 0
    if ($Section.Count -gt 0 -and $Section[0] -match '^## \[') {
        $start = 1
    }

    $body = ($Section | Select-Object -Skip $start | ForEach-Object { $_ }) -join "`n"
    return $body.Trim() + "`n"
}

$versionPattern = "^## \[$([regex]::Escape($ver))\]( - [0-9]{4}-[0-9]{2}-[0-9]{2})?$"
$notes = Get-ChangelogSection -HeadingPattern $versionPattern

if (-not ($notes -and (Test-SectionHasBody $notes))) {
    throw @"
No ## [$ver] section with release notes found in $Changelog for tag $Tag.
Add a dated ## [$ver] - YYYY-MM-DD section (move bullets out of [Unreleased]) before publishing.
"@
}

$text = Format-ReleaseNotesBody -Section $notes
if ($OutFile) {
    Set-Content -LiteralPath $OutFile -Value $text -NoNewline -Encoding utf8
    Write-Host "Wrote $OutFile"
} else {
    Write-Output $text
}
