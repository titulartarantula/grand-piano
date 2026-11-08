# Grand Piano: per-user install (no admin rights needed).
# Copies the app to %LOCALAPPDATA%\Programs\GrandPiano and adds a Start menu shortcut.
# Run from the unzipped folder:  powershell -ExecutionPolicy Bypass -File .\install.ps1
# Optional: -SampleArchive "C:\path\SalamanderGrandPiano-SFZ+FLAC-V3+20200602.tar.gz"
#           installs the samples from a local copy instead of downloading them on first launch.
param([string]$SampleArchive = "")

$ErrorActionPreference = "Stop"
$src = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA "Programs\GrandPiano"
New-Item -ItemType Directory -Force -Path $dest | Out-Null

# Stop a running copy so the exe can be replaced
Get-Process -Name "GrandPiano" -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300

foreach ($f in @("GrandPiano.exe", "README.md", "LICENSE-SAMPLES.txt", "uninstall.ps1")) {
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
$s.IconLocation = (Join-Path $dest "GrandPiano.exe") + ",0"
$s.Save()

if ($SampleArchive -ne "") {
    & (Join-Path $dest "GrandPiano.exe") --download-samples --archive "$SampleArchive" | Out-Host
}

Write-Host "Installed to $dest"
Write-Host "Start menu shortcut: $lnk"
