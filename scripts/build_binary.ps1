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
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        Write-Host "Build failed (cargo exit $code)." -ForegroundColor Red
        $exe = Join-Path $RepoRoot 'target\release\rustdl.exe'
        if (Test-Path -LiteralPath $exe) {
            Write-Host "An older binary is still at: $exe"
        }
        if (Get-Process -Name rustdl -ErrorAction SilentlyContinue) {
            Write-Host "rustdl is still running (check the system tray). Stop it, then rebuild:"
            Write-Host '  Stop-Process -Name rustdl -Force -ErrorAction SilentlyContinue'
        } else {
            Write-Host @"
If cargo reported "Access is denied" replacing rustdl.exe, something still has the file open
(e.g. a PATH symlink to target\release\rustdl.exe, antivirus, or another cargo run).
"@
        }
        exit $code
    }

    $exe = Join-Path $RepoRoot 'target\release\rustdl.exe'
    if (Test-Path -LiteralPath $exe) {
        Write-Host "Built: $exe"
    } else {
        $alt = Join-Path $RepoRoot 'target\release\rustdl'
        if (Test-Path -LiteralPath $alt) {
            Write-Host "Built: $alt"
        } else {
            Write-Error 'cargo succeeded but no release binary was found under target\release\'
        }
    }
    exit 0
}
finally {
    Pop-Location
}
