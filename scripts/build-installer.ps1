# Builds the release binaries and the installer, dist\BDownloader-Setup-<version>.exe.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
#
# -Quick builds with the `quick` profile instead: seconds instead of minutes
# after a change, for installers to test locally. Releases use the default.
#
# Needs Inno Setup 6 (winget install JRSoftware.InnoSetup).

param([switch]$Quick)

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
    $buildProfile = if ($Quick) { 'quick' } else { 'release' }
    cargo build --profile $buildProfile --workspace
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    # The Firefox add-on, signed by Mozilla so Firefox keeps it. Signed once
    # per version; later builds of the same version reuse the signed file.
    # Test builds never sign: Mozilla signs a version only once, and the
    # release must get the final code. They reuse a signed file if there is one.
    $signed = Join-Path $root "dist\bdownloader-firefox-$version.xpi"
    if (-not $Quick) {
        & (Join-Path $PSScriptRoot 'sign-firefox.ps1')
    } elseif (-not (Test-Path $signed)) {
        & (Join-Path $PSScriptRoot 'firefox-extension.ps1') | Out-Null
        # Fine for a test installer, but Firefox refuses an unsigned add-on.
        Write-Warning "Version $version of the Firefox add-on isn't signed yet; the test installer has it unsigned"
        $signed = Join-Path $root "dist\bdownloader-firefox-$version-unsigned.xpi"
    }
    Copy-Item $signed (Join-Path $root 'dist\bdownloader-firefox.xpi') -Force
    & $iscc /Qp "/DAppVersion=$version" "/DBinDir=$root\target\$buildProfile" (Join-Path $root 'installer\bdownloader.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }
} finally {
    Pop-Location
}
Get-Item (Join-Path $root "dist\BDownloader-Setup-$version.exe") | ForEach-Object { "Built $($_.FullName) ($([math]::Round($_.Length / 1MB, 2)) MB)" }
