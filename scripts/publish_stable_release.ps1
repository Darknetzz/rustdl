#requires -Version 5.1
<#
.SYNOPSIS
  Tag and publish a stable GitHub release for the Cargo.toml version at Commit.

  Skips when rustdl-vX.Y.Z already exists on GitHub. Invoked after dev pushes
  (see wait_and_publish_dev.ps1 / push_dev.ps1) alongside the rolling rustdl-dev build.

.EXAMPLE
  .\scripts\publish_stable_release.ps1
  .\scripts\publish_stable_release.ps1 -DryRun
  .\scripts\publish_stable_release.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [switch] $DryRun,
    [switch] $SkipBuild,
    [string] $Repo = 'Darknetzz/rustdl',
    [string] $Remote = 'github',
    [string] $Commit = '',
    [switch] $AllowDirty,
    [switch] $Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$NotesFile = Join-Path $RepoRoot 'release-notes.md'

function Get-CargoVersionAtCommit {
    param([string] $Sha)
    $spec = '{0}:Cargo.toml' -f $Sha
    $text = (git show $spec 2>$null | Out-String)
    if (-not $text) {
        throw "Could not read Cargo.toml at $Sha"
    }
    if ($text -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"') {
        throw "Could not parse version from Cargo.toml at $Sha"
    }
    return $Matches[1]
}

function Test-GhReleaseExists {
    param([string] $TagName)
    gh release view $TagName --repo $Repo 2>$null | Out-Null
    return $LASTEXITCODE -eq 0
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

function Ensure-AnnotatedTag {
    param(
        [string] $TagName,
        [string] $Sha,
        [string] $Version
    )

    $local = git rev-parse -q --verify "refs/tags/$TagName" 2>$null
    if ($local) {
        if ($local.Trim() -eq $Sha) {
            return
        }
        if (-not $Force) {
            throw "Tag $TagName exists at $(git rev-parse --short $local) but commit is $(git rev-parse --short $Sha). Pass -Force to move the tag."
        }
        if ($DryRun) {
            Write-Host "Would move tag $TagName to $(git rev-parse --short $Sha)"
            return
        }
        git tag -f -a $TagName $Sha -m "rustdl $Version"
        if ($LASTEXITCODE -ne 0) { throw "git tag -f failed for $TagName" }
        return
    }

    if ($DryRun) {
        Write-Host "Would create tag $TagName at $(git rev-parse --short $Sha)"
        return
    }

    git tag -a $TagName $Sha -m "rustdl $Version"
    if ($LASTEXITCODE -ne 0) { throw "git tag failed for $TagName" }
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
    $version = Get-CargoVersionAtCommit -Sha $Commit
    $tag = "rustdl-v$version"
    $title = "rustdl $version"

    if ((git status --porcelain) -and -not $AllowDirty) {
        throw 'Working tree is not clean. Commit/stash or pass -AllowDirty.'
    }

    Write-Host 'Stable release'
    Write-Host "  Repo:    $Repo"
    Write-Host "  Tag:     $tag"
    Write-Host "  Commit:  $shortCommit ($Commit)"
    Write-Host "  Version: $version"

    if (Test-GhReleaseExists -TagName $tag) {
        if (-not $Force) {
            Write-Host ''
            Write-Host "GitHub release $tag already exists; skipping stable publish."
            exit 0
        }
        Write-Host ''
        Write-Host "Release $tag exists; -Force will refresh assets and target."
    }

    Ensure-AnnotatedTag -TagName $tag -Sha $Commit -Version $version

    if (-not $SkipBuild -and -not $DryRun) {
        Write-Host ''
        Write-Host 'Building release binary...'
        & (Join-Path $PSScriptRoot 'build_binary.ps1')
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } elseif (-not $SkipBuild -and $DryRun) {
        Write-Host ''
        Write-Host 'Would run: .\scripts\build_binary.ps1'
    }

    & (Join-Path $PSScriptRoot 'extract_release_notes.ps1') -Tag $tag -OutFile $NotesFile
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if ($DryRun) {
        $binaryHint = Join-Path $RepoRoot 'target\release\rustdl.exe'
        if (-not (Test-Path -LiteralPath $binaryHint)) {
            $binaryHint = Join-Path $RepoRoot 'target/release/rustdl'
        }
        Write-Host ''
        Write-Host "Dry run: would publish stable release $tag"
        Write-Host "  Push tag: git push $Remote $tag"
        Write-Host "  Binary:   $binaryHint"
        Write-Host "  Notes:    $NotesFile"
        exit 0
    }

    $localTagSha = (git rev-parse -q --verify "refs/tags/$tag" 2>$null).Trim()
    $remoteTag = (
        git ls-remote $Remote "refs/tags/$tag" 2>$null |
        ForEach-Object { ($_ -split '\s+')[0] } |
        Select-Object -First 1
    )

    if (-not $remoteTag) {
        Write-Host ''
        Write-Host "Pushing tag $tag to $Remote ..."
        git push $Remote $tag
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } elseif ($remoteTag -ne $localTagSha) {
        if (-not $Force) {
            throw "Remote tag $tag ($remoteTag) differs from local ($localTagSha). Pass -Force to update."
        }
        Write-Host ''
        Write-Host "Force-pushing tag $tag to $Remote ..."
        git push --force $Remote $tag
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    $binary = Get-ReleaseBinaryPath

    if (Test-GhReleaseExists -TagName $tag) {
        Write-Host ''
        Write-Host "Updating existing release $tag ..."
        gh release edit $tag `
            --repo $Repo `
            --title $title `
            --notes-file $NotesFile `
            --target $Commit
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

        gh release upload $tag $binary --repo $Repo --clobber
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } else {
        Write-Host ''
        Write-Host "Creating release $tag ..."
        gh release create $tag `
            --repo $Repo `
            --title $title `
            --notes-file $NotesFile `
            --target $Commit `
            $binary
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    $url = "https://github.com/$Repo/releases/tag/$tag"
    Write-Host ''
    Write-Host "Published stable release: $url"
}
finally {
    Pop-Location
}
