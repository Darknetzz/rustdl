# Capture README screenshots (desktop + web UI helper).
# Requires: release binary, Python 3, Pillow, pywin32 (Windows desktop capture).
# Optional: playwright + chromium for automated web UI capture.
#
# Usage (from repo root):
#   .\scripts\capture_readme_screenshots.ps1
#   .\scripts\capture_readme_screenshots.ps1 -SkipWeb
#   .\scripts\capture_readme_screenshots.ps1 -SkipDesktop

param(
    [switch]$SkipDesktop,
    [switch]$SkipWeb,
    [int]$Port = 8765
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

$argsList = @("scripts/capture_readme_screenshots.py")
if ($SkipDesktop) { $argsList += "--skip-desktop" }
if ($SkipWeb) { $argsList += "--skip-web" }
if ($Port -ne 8765) { $argsList += @("--port", $Port) }

python @argsList
