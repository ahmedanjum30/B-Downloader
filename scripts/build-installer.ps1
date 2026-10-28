# Builds the release binaries and the installer, dist\BDownloader-Setup-<version>.exe.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
#
# -Quick builds with the `quick` profile instead: seconds instead of minutes
# after a change, for installers to test locally. Releases use the default.
#
# Needs B-Installer's bi.exe and bi-setup.exe on PATH, side by side: download both from
# https://github.com/ahmedanjum30/B-Installer/releases/latest, or
#   cargo install --git https://github.com/ahmedanjum30/B-Installer bi setup

param([switch]$Quick)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

$bi = (Get-Command bi -ErrorAction SilentlyContinue).Source
if (-not $bi) { throw 'B-Installer not found. Get it from https://github.com/ahmedanjum30/B-Installer/releases, or: cargo install --git https://github.com/ahmedanjum30/B-Installer bi setup' }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

Push-Location $root
try {
    $buildProfile = if ($Quick) { 'quick' } else { 'release' }
    cargo build --profile $buildProfile --workspace
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    & $bi (Join-Path $root 'installer\bdownloader.toml') -D "version=$version" -D "bindir=$root\target\$buildProfile"
    if ($LASTEXITCODE -ne 0) { throw 'B-Installer failed' }
} finally {
    Pop-Location
}
Get-Item (Join-Path $root "dist\BDownloader-Setup-$version.exe") | ForEach-Object { "Built $($_.FullName) ($([math]::Round($_.Length / 1MB, 2)) MB)" }
