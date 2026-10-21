# Builds the Microsoft Store (MSIX) package, dist\BDownloader-<version>.msix,
# from the release binaries and installer\msix\AppxManifest.xml.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-msix.ps1 [-Quick] [-SignForTest]
#
# The Store signs the package itself, so a Store upload is unsigned. With
# -SignForTest it's signed with a self-made test certificate instead, to
# install it on this PC (the certificate must be trusted once; see below).
#
# Needs the Windows SDK (makeappx.exe and signtool.exe).

param([switch]$Quick, [switch]$SignForTest)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $root 'dist'

# The package identity Partner Center gives the MSIX product (Product
# management > Product identity).
$identity = 'AhmadAnjum.B-Downloader'
$publisher = 'CN=69DABA06-A98B-493E-B856-B08EE2FDB592'

$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName 'x64\makeappx.exe') } | Sort-Object Name | Select-Object -Last 1
if (-not $sdk) { throw 'Windows SDK not found (makeappx.exe). Install it with: winget install Microsoft.WindowsSDK.10.0.26100' }
$makeappx = Join-Path $sdk.FullName 'x64\makeappx.exe'
$signtool = Join-Path $sdk.FullName 'x64\signtool.exe'

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$buildProfile = if ($Quick) { 'quick' } else { 'release' }
Push-Location $root
try {
    cargo build --profile $buildProfile -p bdownloader -p bdownloader-host
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally {
    Pop-Location
}

# The package's files, laid out as they'll be installed.
$layout = Join-Path $dist 'msix'
if (Test-Path $layout) { Remove-Item -Recurse -Force $layout }
New-Item -ItemType Directory -Force $layout | Out-Null
Copy-Item (Join-Path $root "target\$buildProfile\BDownloader.exe"), (Join-Path $root "target\$buildProfile\bdl-host.exe") $layout
Copy-Item -Recurse (Join-Path $root 'extension') (Join-Path $layout 'extension')
Copy-Item -Recurse (Join-Path $root 'installer\msix\Assets') (Join-Path $layout 'Assets')
Copy-Item (Join-Path $root 'LICENSE') (Join-Path $layout 'LICENSE.txt')
Copy-Item (Join-Path $root 'PRIVACY.md') $layout
$manifest = [IO.File]::ReadAllText((Join-Path $root 'installer\msix\AppxManifest.xml'))
$manifest = $manifest.Replace('{{IDENTITY}}', $identity).Replace('{{PUBLISHER}}', $publisher).Replace('{{VERSION}}', "$version.0")
[IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding $false))

# The index of the icons' size variants (resources.pri); without it Windows
# only uses the plain icons, drawn on a plate in the accent color.
$makepri = Join-Path $sdk.FullName 'x64\makepri.exe'
$priconfig = Join-Path $dist 'priconfig.xml'
& $makepri createconfig /cf $priconfig /dq en-US /pv 10.0.0 /o | Out-Null
& $makepri new /pr $layout /cf $priconfig /mn (Join-Path $layout 'AppxManifest.xml') /of (Join-Path $layout 'resources.pri') /o | Select-Object -Last 1
if ($LASTEXITCODE -ne 0) { throw 'makepri failed' }

$package = Join-Path $dist "BDownloader-$version.msix"
if (Test-Path $package) { Remove-Item $package }
& $makeappx pack /d $layout /p $package /o | Select-Object -Last 2
if ($LASTEXITCODE -ne 0) { throw 'makeappx failed' }

if ($SignForTest) {
    # A test certificate with the package's publisher name, kept in the
    # current user's store. To install the package, trust it once (as admin):
    #   Import-Certificate -FilePath dist\BDownloader-test.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople
    $cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq $publisher -and $_.NotAfter -gt (Get-Date) } | Select-Object -First 1
    if (-not $cert) {
        $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -FriendlyName 'BDownloader MSIX test' `
            -CertStoreLocation Cert:\CurrentUser\My -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
    }
    Export-Certificate -Cert $cert -FilePath (Join-Path $dist 'BDownloader-test.cer') | Out-Null
    & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $package | Select-Object -Last 1
    if ($LASTEXITCODE -ne 0) { throw 'signtool failed' }
}
Get-Item $package | ForEach-Object { "Built $($_.FullName) ($([math]::Round($_.Length / 1MB, 2)) MB)" }
