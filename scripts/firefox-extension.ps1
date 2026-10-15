# Builds the Firefox version of the browser extension from the same files:
# dist\firefox-extension\ (for trying changes with "Load Temporary Add-on")
# and dist\bdownloader-firefox-<version>-unsigned.xpi (for Mozilla to sign,
# scripts\sign-firefox.ps1).
#
#   powershell -ExecutionPolicy Bypass -File scripts\firefox-extension.ps1
#
# Only the manifest differs: Firefox needs an add-on ID, loads background
# scripts from a list instead of a service worker, and doesn't know some
# Chromium-only keys.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

# Firefox add-on ID. The app's bridge registration for Firefox allows this ID.
$addonId = 'bdownloader@ahmedanjum30'

$root = Split-Path $PSScriptRoot -Parent
$source = Join-Path $root 'extension'
$dist = Join-Path $root 'dist'
$folder = Join-Path $dist 'firefox-extension'

$manifest = [IO.File]::ReadAllText((Join-Path $source 'manifest.json')) | ConvertFrom-Json
$manifest.PSObject.Properties.Remove('key')
# The service worker's importScripts loads these first; Firefox loads them in this order.
$manifest.background = [pscustomobject]@{ scripts = @('videos.js', 'yt-capture.js', 'background.js') }
foreach ($script in $manifest.content_scripts) {
    $script.PSObject.Properties.Remove('match_origin_as_fallback')
}
$manifest | Add-Member -NotePropertyName browser_specific_settings -NotePropertyValue ([pscustomobject]@{
    gecko = [pscustomobject]@{
        id = $addonId
        # 128 runs content scripts in the page itself ("world": "MAIN"); 140
        # reads data_collection_permissions, which Mozilla requires.
        strict_min_version = '140.0'
        data_collection_permissions = [pscustomobject]@{ required = @('none') }
        # Firefox checks this daily and installs newer versions it lists. The
        # release workflow attaches the file to each GitHub release; "latest"
        # always leads to the newest one.
        update_url = 'https://github.com/ahmedanjum30/BDownloader/releases/latest/download/firefox-updates.json'
    }
})
$json = $manifest | ConvertTo-Json -Depth 10

if (Test-Path $folder) { Remove-Item -Recurse -Force $folder }
New-Item -ItemType Directory -Force $folder | Out-Null
Get-ChildItem $source -Recurse -File | Where-Object { $_.Name -ne 'manifest.json' } | ForEach-Object {
    $target = Join-Path $folder $_.FullName.Substring($source.Length + 1)
    New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
    Copy-Item $_.FullName $target
}
[IO.File]::WriteAllText((Join-Path $folder 'manifest.json'), $json, (New-Object Text.UTF8Encoding $false))

$xpi = Join-Path $dist "bdownloader-firefox-$($manifest.version)-unsigned.xpi"
if (Test-Path $xpi) { Remove-Item $xpi }
$zip = [IO.Compression.ZipFile]::Open($xpi, 'Create')
try {
    # Zip entries must use forward slashes; Compress-Archive in PowerShell 5 writes backslashes.
    Get-ChildItem $folder -Recurse -File | ForEach-Object {
        $name = $_.FullName.Substring($folder.Length + 1).Replace('\', '/')
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $name) | Out-Null
    }
} finally {
    $zip.Dispose()
}
"Wrote $folder and $xpi"
