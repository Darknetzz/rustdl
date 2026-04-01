#requires -Version 5.1
<#
.SYNOPSIS
  Build a single-file pydl executable via flet pack (recommended for Flet apps).

.DESCRIPTION
  Thin wrapper: runs scripts/build_binary.py from the repository root.
  Same arguments as the Python script; see that file or README.

.EXAMPLE
  .\scripts\build_binary.ps1
.EXAMPLE
  .\scripts\build_binary.ps1 --distpath release -n pydl
.EXAMPLE
  .\scripts\build_binary.ps1 -- --icon .\assets\app.ico
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Driver = [System.IO.Path]::Combine($RepoRoot, 'scripts', 'build_binary.py')

if (-not (Test-Path -LiteralPath $Driver)) {
    Write-Error "Missing $Driver — run from a clone of the pydl repo."
}

Push-Location -LiteralPath $RepoRoot
try {
    python $Driver @args
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
