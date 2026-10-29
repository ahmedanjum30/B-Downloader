# Submits a new version of the extension to Edge Add-ons, so Edge updates it
# for everyone once Microsoft has reviewed it.
#
#   powershell -ExecutionPolicy Bypass -File scripts\publish-edge.ps1 [-Zip <path>]
#
# Needs the Publish API credentials from Partner Center (the extension's
# page → Publish API) in EDGE_CLIENT_ID, EDGE_API_KEY and EDGE_PRODUCT_ID.
# Only submits when the extension's version differs from the previous
# release's, so an unchanged extension isn't sent for review again.

param([string]$Zip)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = ([IO.File]::ReadAllText((Join-Path $root 'extension\manifest.json')) | ConvertFrom-Json).version

$clientId = $env:EDGE_CLIENT_ID
$apiKey = $env:EDGE_API_KEY
$productId = $env:EDGE_PRODUCT_ID
if (-not $clientId -or -not $apiKey -or -not $productId) {
    'Edge Add-ons credentials not set; skipping.'
    return
}

# The extension's version in the release before this one.
$previousTag = git -C $root describe --tags --abbrev=0 'HEAD^' 2>$null
if ($previousTag) {
    $previous = (git -C $root show "${previousTag}:extension/manifest.json" | Out-String | ConvertFrom-Json).version
    if ($previous -eq $version) {
        "Extension $version is unchanged since $previousTag; nothing to submit to Edge."
        return
    }
}

if (-not $Zip) { $Zip = Join-Path $root "dist\bdownloader-extension-$version.zip" }
if (-not (Test-Path $Zip)) { throw "$Zip not found; build it with scripts\package-extension.ps1." }

$api = "https://api.addons.microsoftedge.microsoft.com/v1/products/$productId/submissions"
$headers = @{ Authorization = "ApiKey $apiKey"; 'X-ClientID' = $clientId }

# Each step answers with an operation to wait for.
function Wait-Operation($url) {
    for ($i = 0; $i -lt 100; $i++) {
        Start-Sleep 5
        $status = Invoke-RestMethod -Uri $url -Headers $headers
        if ($status.status -eq 'Succeeded') { return $status }
        if ($status.status -eq 'Failed') {
            throw "Edge Add-ons: $($status.message) $(($status.errors | ForEach-Object { $_.message }) -join ' ')"
        }
    }
    throw 'Edge Add-ons is taking unusually long.'
}

"Uploading $Zip (extension $version) to Edge Add-ons"
$upload = Invoke-WebRequest -Method Post -Uri "$api/draft/package" -Headers $headers -ContentType 'application/zip' -InFile $Zip -UseBasicParsing
Wait-Operation "$api/draft/package/operations/$($upload.Headers.Location)" | Out-Null

# Microsoft asks for these with every submission.
$notes = @'
No accounts or credentials are needed. The extension depends on one other product: BDownloader, a free open-source download manager for Windows. Without the app installed, downloads simply stay in the browser.

To test:
1. Install the app: https://github.com/ahmedanjum30/B-Downloader/releases/latest (BDownloader-Setup-*.exe; installs for the current user, no admin rights needed). Windows SmartScreen may warn because the installer isn't code-signed yet: click More info, then Run anyway.
2. Start a download in Edge, for example https://proof.ovh.net/files/100Mb.dat. BDownloader opens and takes the download over.
3. Right-click any link and choose "Download with BDownloader".
4. Open a page with a video (for example a YouTube video, and let it play for a few seconds) and hover over the video: a "Download this video" bar appears with the qualities available.

The extension talks only to the BDownloader app on the same computer, through the native messaging host com.bdownloader.host. No data is sent to any server. Source code: https://github.com/ahmedanjum30/B-Downloader
'@
"Submitting extension $version for review"
$body = @{ notes = $notes } | ConvertTo-Json
$submit = Invoke-WebRequest -Method Post -Uri $api -Headers $headers -ContentType 'application/json' -Body ([Text.Encoding]::UTF8.GetBytes($body)) -UseBasicParsing
Wait-Operation "$api/operations/$($submit.Headers.Location)" | Out-Null
"Extension $version submitted to Edge Add-ons; Edge updates it once Microsoft approves it."
