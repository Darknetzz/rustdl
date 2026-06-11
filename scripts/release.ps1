#requires -Version 5.1
<#
.SYNOPSIS
  Cut a rustdl release: finalize CHANGELOG, commit, tag rustdl-vX.Y.Z, optionally push.

.EXAMPLE
  .\scripts\release.ps1 -DryRun
  .\scripts\release.ps1
  .\scripts\release.ps1 -Push -Yes
#>
[CmdletBinding()]
param(
    [switch] $DryRun,
    [switch] $SkipChecks,
    [switch] $Push,
    [string] $Remote = 'github',
    [switch] $Yes
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$GithubRepo = 'Darknetzz/rustdl'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$ChangelogPath = Join-Path $RepoRoot 'CHANGELOG.md'
$CargoToml = Join-Path $RepoRoot 'Cargo.toml'

function Get-CargoVersion {
    $content = Get-Content -LiteralPath $CargoToml -Raw
    if ($content -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"') {
        throw 'Could not parse version from Cargo.toml'
    }
    return $Matches[1]
}

function Get-LatestReleasedVersion {
    $versions = Select-String -LiteralPath $ChangelogPath -Pattern '^## \[(\d+\.\d+\.\d+)\]' |
        ForEach-Object { $_.Matches[0].Groups[1].Value } |
        Sort-Object { [version]$_ }
    if (-not $versions) {
        throw 'Could not find a previous released version in CHANGELOG.md'
    }
    return $versions[-1]
}

function Test-UnreleasedHasContent {
    $lines = Get-Content -LiteralPath $ChangelogPath
    $inUnreleased = $false
    foreach ($line in $lines) {
        if ($line -eq '## [Unreleased]') {
            $inUnreleased = $true
            continue
        }
        if ($inUnreleased -and $line -match '^## \[') {
            break
        }
        if ($inUnreleased -and $line.Trim().Length -gt 0) {
            return $true
        }
    }
    return $false
}

function Update-ChangelogForRelease {
    param(
        [string] $Version,
        [string] $Date,
        [string] $Previous
    )

    $text = [IO.File]::ReadAllText($ChangelogPath)
    if ($text -notmatch '(?ms)^## \[Unreleased\]\r?\n(.*?)(?=^## \[)') {
        throw 'Could not find [Unreleased] section in CHANGELOG.md'
    }

    $body = $Matches[1].TrimEnd()
    if ([string]::IsNullOrWhiteSpace($body)) {
        throw '[Unreleased] in CHANGELOG.md is empty; nothing to release.'
    }

    $replacement = "## [Unreleased]`r`n`r`n## [$Version] - $Date`r`n`r`n$body`r`n`r`n"
    $text = [regex]::Replace($text, '(?ms)^## \[Unreleased\]\r?\n.*?(?=^## \[)', $replacement)

    $unreleasedLink = "[Unreleased]: https://github.com/$GithubRepo/compare/rustdl-v${Version}...dev"
    $versionLink = "[$Version]: https://github.com/$GithubRepo/compare/rustdl-v${Previous}...rustdl-v${Version}"

    if ($text -match '\[Unreleased\]:') {
        $text = [regex]::Replace($text, '\[Unreleased\]:[^\r\n]*', $unreleasedLink)
    } else {
        $text = $text.TrimEnd() + "`r`n`r`n$unreleasedLink`r`n"
    }

    if ($text -notmatch "\[$([regex]::Escape($Version))\]:") {
        $text = [regex]::Replace($text, '\[Unreleased\]:', "$versionLink`r`n[Unreleased]:")
    }

    [IO.File]::WriteAllText($ChangelogPath, $text)
}

Push-Location -LiteralPath $RepoRoot
try {
    $version = Get-CargoVersion
    $tag = "rustdl-v$version"
    $date = Get-Date -Format 'yyyy-MM-dd'
    $prev = Get-LatestReleasedVersion

    if (-not (Test-UnreleasedHasContent)) {
        throw '[Unreleased] in CHANGELOG.md is empty; nothing to release.'
    }

    $existingTag = git tag -l $tag 2>$null
    if ($existingTag) {
        Write-Host "Tag $tag exists (from version bump); will move to the release commit."
    }

    $branch = git rev-parse --abbrev-ref HEAD
    if ($branch -ne 'dev') {
        Write-Warning "Not on dev branch (on $branch)."
    }

    if ((git status --porcelain) -and -not $DryRun) {
        throw 'Working tree is not clean. Commit or stash changes before releasing.'
    }

    Write-Host 'Release plan'
    Write-Host "  Version:     $version"
    Write-Host "  Tag:         $tag"
    Write-Host "  Date:        $date"
    Write-Host "  Previous:    $prev"
    Write-Host "  Compare:     rustdl-v${prev}...rustdl-v${version}"
    if ($Push) {
        Write-Host "  Remote push: $Remote (dev + tag)"
    } else {
        Write-Host '  Remote push: no (local commit + tag only)'
    }

    if (-not $SkipChecks -and -not $DryRun) {
        Write-Host ''
        Write-Host 'Running pre-release checks (fmt, clippy, test)...'
        cargo fmt --all -- --check
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        cargo clippy --all-targets --all-features -- -D warnings
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        cargo test --all-targets --all-features
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } elseif (-not $SkipChecks -and $DryRun) {
        Write-Host ''
        Write-Host 'Would run: cargo fmt --check, clippy, test'
    }

    if ($DryRun) {
        Write-Host ''
        Write-Host 'Dry run: no files modified, no commit, no tag.'
        exit 0
    }

    if (-not $Yes) {
        $reply = Read-Host 'Proceed with release commit and tag? [y/N]'
        if ($reply -notmatch '^(y|yes)$') {
            Write-Host 'Aborted.'
            exit 1
        }
    }

    Update-ChangelogForRelease -Version $version -Date $date -Previous $prev

    git add CHANGELOG.md
    git commit -m "release: v$version"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if (git rev-parse -q --verify "refs/tags/$tag" 2>$null) {
        Write-Host "Tag $tag exists (from a version bump); moving to release commit."
        git tag -f -a $tag -m "release: rustdl $version"
    } else {
        git tag -a $tag -m "rustdl $version"
    }
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Host ''
    Write-Host "Created commit and tag $tag."
    Write-Host 'Next:'
    Write-Host "  git push $Remote dev"
    Write-Host "  git push $Remote $tag"

    if ($Push) {
        git push $Remote dev
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        git push $Remote $tag
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Write-Host "Pushed dev and $tag to $Remote."
    }
}
finally {
    Pop-Location
}
