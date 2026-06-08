#requires -Version 5.1
<#
.SYNOPSIS
  Build a release rustdl binary with Cargo.

.EXAMPLE
  .\scripts\build_binary.ps1
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $RepoRoot
try {
    cargo build --release
    $exe = Join-Path $RepoRoot 'target\release\rustdl.exe'
    if (Test-Path -LiteralPath $exe) {
        Write-Host "Built: $exe"
    } else {
        $alt = Join-Path $RepoRoot 'target\release\rustdl'
        if (Test-Path -LiteralPath $alt) { Write-Host "Built: $alt" }
    }
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
