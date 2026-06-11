#requires -Version 5.1
<#
.SYNOPSIS
  Run the full local CI checklist (replaces GitHub Actions for day-to-day dev).

.EXAMPLE
  .\scripts\ci_local.ps1
  .\scripts\ci_local.ps1 -SkipDeny -SkipAudit
#>
[CmdletBinding()]
param(
    [switch] $SkipDeny,
    [switch] $SkipAudit
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $RepoRoot
try {
    Write-Host 'ci_local: cargo fmt --check'
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Host 'ci_local: cargo clippy'
    cargo clippy --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    Write-Host 'ci_local: cargo test'
    cargo test --all-targets --all-features
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if (-not $SkipDeny) {
        if (-not (Get-Command cargo-deny -ErrorAction SilentlyContinue)) {
            Write-Host 'ci_local: installing cargo-deny'
            cargo install cargo-deny --locked
            if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        }
        Write-Host 'ci_local: cargo deny'
        cargo deny check
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    if (-not $SkipAudit) {
        if (-not (Get-Command cargo-audit -ErrorAction SilentlyContinue)) {
            Write-Host 'ci_local: installing cargo-audit'
            cargo install cargo-audit --locked
            if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        }
        Write-Host 'ci_local: cargo audit'
        cargo audit
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    Write-Host 'ci_local: all checks passed.'
    exit 0
}
finally {
    Pop-Location
}
