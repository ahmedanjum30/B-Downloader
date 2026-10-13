# Builds the release binaries and the installer, dist\BDownloader-Setup-<version>.exe.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
#
# Needs Inno Setup 6 (winget install JRSoftware.InnoSetup).

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue).Source,
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup' }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

Push-Location $root
try {
    cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    & $iscc /Qp "/DAppVersion=$version" "/DBinDir=$root\target\release" (Join-Path $root 'installer\bdownloader.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }
} finally {
    Pop-Location
}
Get-Item (Join-Path $root "dist\BDownloader-Setup-$version.exe") | ForEach-Object { "Built $($_.FullName) ($([math]::Round($_.Length / 1MB, 2)) MB)" }
