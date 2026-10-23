# Submits a new version of the Firefox add-on to Firefox Add-ons
# (addons.mozilla.org), where it's listed at
# https://addons.mozilla.org/firefox/addon/bdownloader/. Mozilla reviews it
# and Firefox updates the add-on for everyone once it's approved.
#
#   powershell -ExecutionPolicy Bypass -File scripts\publish-firefox.ps1
#
# Needs the API credentials from https://addons.mozilla.org/developers/addon/api/key/
# in AMO_JWT_ISSUER and AMO_JWT_SECRET. The extension has its own version,
# raised only when it changes: an unchanged extension isn't submitted again,
# and one that changed without a new version stops the release.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$root = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $root 'dist'
& (Join-Path $PSScriptRoot 'firefox-extension.ps1') | Out-Null
$manifest = [IO.File]::ReadAllText((Join-Path $dist 'firefox-extension\manifest.json')) | ConvertFrom-Json
$version = $manifest.version
$addonId = $manifest.browser_specific_settings.gecko.id
$xpi = Join-Path $dist "bdownloader-firefox-$version-unsigned.xpi"

# A changed extension needs a new version: Mozilla never takes a version twice.
$previousTag = git -C $root describe --tags --abbrev=0 'HEAD^' 2>$null
if ($previousTag) {
    $previous = (git -C $root show "${previousTag}:extension/manifest.json" | Out-String | ConvertFrom-Json).version
    $changed = git -C $root diff --name-only $previousTag HEAD -- extension
    if ($changed -and $previous -eq $version) {
        $changed | ForEach-Object { "  $_" }
        throw "The extension changed since $previousTag (files above) but is still version $version. Raise `"version`" in extension\manifest.json."
    }
}

$issuer = if ($env:AMO_JWT_ISSUER) { $env:AMO_JWT_ISSUER } else { [Environment]::GetEnvironmentVariable('AMO_JWT_ISSUER', 'User') }
$secret = if ($env:AMO_JWT_SECRET) { $env:AMO_JWT_SECRET } else { [Environment]::GetEnvironmentVariable('AMO_JWT_SECRET', 'User') }
if (-not $issuer -or -not $secret) {
    'Firefox Add-ons credentials not set; skipping.'
    return
}
$api = 'https://addons.mozilla.org/api/v5/addons'
$addon = "$api/addon/$([Uri]::EscapeDataString($addonId))"

function Base64Url([byte[]]$bytes) {
    [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
}

# Each request needs a fresh token, valid for at most five minutes.
function Token {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $header = Base64Url ([Text.Encoding]::UTF8.GetBytes('{"alg":"HS256","typ":"JWT"}'))
    $claims = @{ iss = $issuer; jti = [guid]::NewGuid().ToString(); iat = $now; exp = $now + 60 } | ConvertTo-Json -Compress
    $payload = Base64Url ([Text.Encoding]::UTF8.GetBytes($claims))
    $hmac = New-Object Security.Cryptography.HMACSHA256 (, [Text.Encoding]::UTF8.GetBytes($secret))
    $signature = Base64Url ($hmac.ComputeHash([Text.Encoding]::UTF8.GetBytes("$header.$payload")))
    "JWT $header.$payload.$signature"
}

function Call($method, $url, $body) {
    $params = @{ Method = $method; Uri = $url; Headers = @{ Authorization = (Token) } }
    if ($body) {
        $params.Body = [Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Compress -Depth 5))
        $params.ContentType = 'application/json'
    }
    Invoke-RestMethod @params
}

# Mozilla already has this version (submitted by an earlier release, listed or not).
$existing = try { Call GET "$addon/versions/v$version/" } catch { $null }
if ($existing) {
    "Firefox Add-ons already has version $version; nothing to submit."
    return
}

"Uploading $xpi (version $version) to Firefox Add-ons"
$client = New-Object Net.Http.HttpClient
$client.Timeout = [TimeSpan]::FromMinutes(5)
$form = New-Object Net.Http.MultipartFormDataContent
$file = New-Object Net.Http.ByteArrayContent (, [IO.File]::ReadAllBytes($xpi))
$file.Headers.ContentType = 'application/x-xpinstall'
$form.Add($file, 'upload', (Split-Path $xpi -Leaf))
$form.Add((New-Object Net.Http.StringContent 'listed'), 'channel')
$request = New-Object Net.Http.HttpRequestMessage ([Net.Http.HttpMethod]::Post), "$api/upload/"
$request.Headers.TryAddWithoutValidation('Authorization', (Token)) | Out-Null
$request.Content = $form
$response = $client.SendAsync($request).Result
$text = $response.Content.ReadAsStringAsync().Result
if (-not $response.IsSuccessStatusCode) { throw "Upload refused ($([int]$response.StatusCode)): $text" }
$upload = $text | ConvertFrom-Json

# Mozilla's automatic checks.
while (-not $upload.processed) {
    Start-Sleep 3
    $upload = Call GET "$api/upload/$($upload.uuid)/"
}
if (-not $upload.valid) {
    $upload.validation.messages | Where-Object { $_.type -eq 'error' } | ForEach-Object { "  $($_.message) ($($_.file))" }
    throw 'Mozilla found problems in the extension (above).'
}

Call POST "$addon/versions/" @{ upload = $upload.uuid } | Out-Null
"Submitted version $version to Firefox Add-ons; Firefox updates the add-on once Mozilla approves it."
