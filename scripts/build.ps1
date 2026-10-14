# Build basecampd.exe and the Windows installer on Windows.
#   powershell -ExecutionPolicy Bypass -File scripts\build.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
cargo build --release -p basecampd
New-Item -ItemType Directory -Force -Path dist\bin\win | Out-Null
Copy-Item target\release\basecampd.exe dist\bin\win\basecampd.exe -Force
Set-Location app
if (-not (Test-Path node_modules)) { npm ci }
npx electron-builder --win --publish never
Set-Location $root
Write-Host "Artifacts are in dist\"
