# Builds the browser extension zip for the Chrome Web Store and Edge Add-ons.
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-extension.ps1
#
# The development `key` is left out of the manifest: each store assigns its
# own extension ID on the first upload. Writes dist\bdownloader-extension-<version>.zip.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$root = Split-Path $PSScriptRoot -Parent
$source = Join-Path $root 'extension'
$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null

$manifestText = [IO.File]::ReadAllText((Join-Path $source 'manifest.json'))
$version = ($manifestText | ConvertFrom-Json).version
# Drop the "key" line, keeping the rest of the file exactly as written.
$storeManifest = ($manifestText -split "`n" | Where-Object { $_ -notmatch '^\s*"key"\s*:' }) -join "`n"
$null = $storeManifest | ConvertFrom-Json  # fails if the result isn't valid JSON

$zipPath = Join-Path $dist "bdownloader-extension-$version.zip"
if (Test-Path $zipPath) { Remove-Item $zipPath }
$zip = [IO.Compression.ZipFile]::Open($zipPath, 'Create')
try {
    # Zip entries must use forward slashes; Compress-Archive in PowerShell 5 writes backslashes.
    $entry = $zip.CreateEntry('manifest.json')
    $writer = New-Object IO.StreamWriter($entry.Open(), (New-Object Text.UTF8Encoding $false))
    $writer.Write($storeManifest)
    $writer.Dispose()
    Get-ChildItem $source -Recurse -File | Where-Object { $_.Name -ne 'manifest.json' } | ForEach-Object {
        $name = $_.FullName.Substring($source.Length + 1).Replace('\', '/')
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $name) | Out-Null
    }
} finally {
    $zip.Dispose()
}

$zip = [IO.Compression.ZipFile]::OpenRead($zipPath)
"Wrote $zipPath"
$zip.Entries | ForEach-Object { "  $($_.FullName)  ($($_.Length) bytes)" }
$zip.Dispose()
