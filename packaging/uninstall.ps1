# Grand Piano: remove the per-user install. Settings (%APPDATA%\GrandPiano) and
# samples (%LOCALAPPDATA%\GrandPiano) are kept unless you pass -All.
param([switch]$All)
$ErrorActionPreference = "Continue"
Get-Process -Name "GrandPiano" -ErrorAction SilentlyContinue | Stop-Process -Force
$dest = Join-Path $env:LOCALAPPDATA "Programs\GrandPiano"
$lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "Grand Piano.lnk"
Remove-Item -LiteralPath $lnk -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $dest -Recurse -Force -ErrorAction SilentlyContinue
if ($All) {
    Remove-Item -LiteralPath (Join-Path $env:APPDATA "GrandPiano") -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $env:LOCALAPPDATA "GrandPiano") -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "Grand Piano removed."
