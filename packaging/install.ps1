# Grand Piano: per-user install (no admin rights needed).
# Copies the app to %LOCALAPPDATA%\Programs\GrandPiano and adds a Start menu shortcut.
# Run from the unzipped folder:  powershell -ExecutionPolicy Bypass -File .\install.ps1
# Optional: -SampleArchive "C:\path\SalamanderGrandPiano-SFZ+FLAC-V3+20200602.tar.gz"
#           installs the samples from a local copy instead of downloading them on first launch.
# If Grand Piano is running, the install stops and asks you to close it first
# (pass -Force to close it automatically).
param([string]$SampleArchive = "", [switch]$Force)

$ErrorActionPreference = "Stop"
$src = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA "Programs\GrandPiano"
New-Item -ItemType Directory -Force -Path $dest | Out-Null

# The exe can't be replaced while it runs. Don't close it behind the player's back.
$running = Get-Process -Name "GrandPiano" -ErrorAction SilentlyContinue
if ($running) {
    if (-not $Force) {
        Write-Host "Grand Piano is running. Close it, then run this again (or add -Force to close it now)."
        exit 1
    }
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 300
}

foreach ($f in @("GrandPiano.exe", "GrandPiano.ico", "README.md", "LICENSE-SAMPLES.txt", "uninstall.ps1")) {
    $p = Join-Path $src $f
    if (Test-Path -LiteralPath $p) { Copy-Item -LiteralPath $p -Destination $dest -Force }
}

$programs = [Environment]::GetFolderPath("Programs")   # per-user Start menu\Programs
$lnk = Join-Path $programs "Grand Piano.lnk"
$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = Join-Path $dest "GrandPiano.exe"
$s.WorkingDirectory = $dest
$s.Description = "Grand Piano: sampled grand piano for your MIDI keyboard"
$ico = Join-Path $dest "GrandPiano.ico"
if (Test-Path -LiteralPath $ico) { $s.IconLocation = $ico + ",0" } else { $s.IconLocation = (Join-Path $dest "GrandPiano.exe") + ",0" }
$s.Save()

if ($SampleArchive -ne "") {
    & (Join-Path $dest "GrandPiano.exe") --download-samples --archive "$SampleArchive" | Out-Host
}

Write-Host "Installed to $dest"
Write-Host "Start menu shortcut: $lnk"
