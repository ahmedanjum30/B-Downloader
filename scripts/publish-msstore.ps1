# Submits a new version of BDownloader to the Microsoft Store: points the
# Store's package at the version's installer on GitHub Pages and submits it
# for certification. The listing text and images stay as they are.
#
#   powershell -ExecutionPolicy Bypass -File scripts\publish-msstore.ps1 [-DryRun]
#
# Needs the Partner Center API app's credentials in MSSTORE_TENANT_ID,
# MSSTORE_CLIENT_ID and MSSTORE_CLIENT_SECRET, and MSSTORE_SELLER_ID. With
# -DryRun it only signs in and shows the current package and status.

param([switch]$DryRun)

$ErrorActionPreference = 'Stop'
$productId = 'XP89FXJJF4J45W'
$root = Split-Path $PSScriptRoot -Parent
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$installer = "https://ahmedanjum30.github.io/B-Downloader/download/$version/BDownloader-Setup-$version.exe"

$tenant = $env:MSSTORE_TENANT_ID
$clientId = $env:MSSTORE_CLIENT_ID
$secret = $env:MSSTORE_CLIENT_SECRET
$seller = $env:MSSTORE_SELLER_ID
if (-not $tenant -or -not $clientId -or -not $secret -or -not $seller) {
    'Microsoft Store credentials not set; skipping.'
    return
}

$token = (Invoke-RestMethod -Method Post "https://login.microsoftonline.com/$tenant/oauth2/v2.0/token" -ContentType 'application/x-www-form-urlencoded' -Body @{
        grant_type = 'client_credentials'
        client_id = $clientId
        client_secret = $secret
        scope = 'https://api.store.microsoft.com/.default'
    }).access_token
$api = "https://api.store.microsoft.com/submission/v1/product/$productId"
$headers = @{ Authorization = "Bearer $token"; 'X-Seller-Account-Id' = $seller }

# Every answer says whether it worked; a refusal lists why.
function Call($method, $path, $body) {
    $params = @{ Method = $method; Uri = "$api$path"; Headers = $headers }
    if ($body) {
        $params.Body = [Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Depth 10 -Compress))
        $params.ContentType = 'application/json'
    }
    $answer = Invoke-RestMethod @params
    if (-not $answer.isSuccess) {
        throw "Microsoft Store: $method $path refused: $(($answer.errors | ForEach-Object { "$($_.code) $($_.message)" }) -join '; ')"
    }
    $answer.responseData
}

$status = Call GET '/status'
$packages = @((Call GET '/packages').packages)
"Store package now: $($packages.packageUrl -join ', ')"
"Store status: ready=$($status.isReady) ongoing submission=$($status.ongoingSubmissionId)"
if ($DryRun) { return }

# One submission at a time: a release during another's certification waits
# for the next release. Store users get new versions from BDownloader's own
# updater meanwhile, so nothing is lost.
if ($status.ongoingSubmissionId) {
    "::warning::A Microsoft Store submission ($($status.ongoingSubmissionId)) is still in progress; $version wasn't submitted. The next release will be."
    return
}
if ($packages.packageUrl -contains $installer) {
    "The Store already has $version."
    return
}

# GitHub Pages publishes the installer a minute or two after the release.
for ($i = 0; ; $i++) {
    try {
        Invoke-WebRequest -Method Head -Uri $installer -UseBasicParsing | Out-Null
        break
    } catch {
        if ($i -ge 40) { throw "$installer isn't available." }
        Start-Sleep 15
    }
}

foreach ($package in $packages) {
    "Pointing package $($package.packageId) to $installer"
    Call PATCH "/packages/$($package.packageId)" @{ packageUrl = $installer } | Out-Null
}
Call POST '/packages/commit' | Out-Null

# The Store downloads and checks the new installer before it can be submitted.
for ($i = 0; ; $i++) {
    $status = Call GET '/status'
    if ($status.isReady) { break }
    if ($i -ge 60) { throw 'The Microsoft Store is taking unusually long to check the package.' }
    Start-Sleep 10
}

$submission = Call POST '/submit'
"Submitted $version to the Microsoft Store (submission $($submission.submissionId)); certification can take a day or more."
