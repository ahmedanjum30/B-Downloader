# Has Mozilla sign the Firefox extension, so Firefox keeps it installed.
# Signed "unlisted": the add-on isn't listed on addons.mozilla.org; we ship
# the signed file ourselves.
#
#   powershell -ExecutionPolicy Bypass -File scripts\sign-firefox.ps1
#
# Needs the API credentials from https://addons.mozilla.org/developers/addon/api/key/
# in the environment variables AMO_JWT_ISSUER and AMO_JWT_SECRET. Builds the
# .xpi first (scripts\firefox-extension.ps1) and writes
# dist\bdownloader-firefox-<version>.xpi, plus dist\firefox-updates.json, which
# tells installed copies about it (see update_url in firefox-extension.ps1).
# Mozilla signs each version number once, so a changed extension needs a new
# version; a signed file already in dist is reused.

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
$signed = Join-Path $dist "bdownloader-firefox-$version.xpi"
$api = 'https://addons.mozilla.org/api/v5/addons'
$versionUrl = "$api/addon/$([Uri]::EscapeDataString($addonId))/versions/v$version/"

# Also read from the user's saved variables, for shells started before they were set.
$issuer = if ($env:AMO_JWT_ISSUER) { $env:AMO_JWT_ISSUER } else { [Environment]::GetEnvironmentVariable('AMO_JWT_ISSUER', 'User') }
$secret = if ($env:AMO_JWT_SECRET) { $env:AMO_JWT_SECRET } else { [Environment]::GetEnvironmentVariable('AMO_JWT_SECRET', 'User') }

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

# The add-on as Mozilla has it, or $null before the first upload.
function Find-Addon {
    try {
        Call GET "$api/addon/$([Uri]::EscapeDataString($addonId))/"
    } catch {
        if ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) { return $null }
        throw
    }
}

# Uploads the file for checking and adds it as a new version.
function Submit {
    "Uploading $xpi"
    $client = New-Object Net.Http.HttpClient
    $client.Timeout = [TimeSpan]::FromMinutes(5)
    $form = New-Object Net.Http.MultipartFormDataContent
    $file = New-Object Net.Http.ByteArrayContent (, [IO.File]::ReadAllBytes($xpi))
    $file.Headers.ContentType = 'application/x-xpinstall'
    $form.Add($file, 'upload', (Split-Path $xpi -Leaf))
    $form.Add((New-Object Net.Http.StringContent 'unlisted'), 'channel')
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
    'Checks passed'

    # The first time, this creates the add-on.
    if (Find-Addon) {
        Call POST "$api/addon/$([Uri]::EscapeDataString($addonId))/versions/" @{ upload = $upload.uuid } | Out-Null
    } else {
        Call POST "$api/addon/" @{ version = @{ upload = $upload.uuid } } | Out-Null
    }
}

if (Test-Path $signed) {
    "Already signed: $signed"
} else {
    if (-not $issuer -or -not $secret) {
        throw 'Set AMO_JWT_ISSUER and AMO_JWT_SECRET (from https://addons.mozilla.org/developers/addon/api/key/).'
    }
    $existing = try { Call GET $versionUrl } catch { $null }
    if ($existing) {
        "Mozilla already has version $version; fetching its signed file"
    } else {
        Submit
    }

    "Waiting for Mozilla to sign version $version"
    for ($i = 0; ; $i++) {
        $info = Call GET $versionUrl
        if ($info.file -and $info.file.status -eq 'public' -and $info.file.url) { break }
        if ($info.file -and $info.file.status -eq 'disabled') { throw "Mozilla refused to sign version $version." }
        if ($i -ge 200) { throw 'Mozilla is taking unusually long; run this again later to pick up the signed file.' }
        Start-Sleep 6
    }
    $download = "$signed.download"
    Invoke-WebRequest -Uri $info.file.url -Headers @{ Authorization = (Token) } -OutFile $download -UseBasicParsing
    Move-Item $download $signed -Force
    "Wrote $signed"
}

# The extension has its own version, raised only when its files change, so an
# unchanged extension isn't uploaded again. A signed file of this version must
# hold exactly our files; if not, the extension changed without a new version.
Add-Type -AssemblyName System.IO.Compression.FileSystem
function Contents($path) {
    $zip = [IO.Compression.ZipFile]::OpenRead($path)
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        $zip.Entries | Where-Object { $_.Name -and -not $_.FullName.StartsWith('META-INF/') } | Sort-Object FullName | ForEach-Object {
            $stream = $_.Open()
            try {
                if ($_.FullName -eq 'manifest.json') {
                    # Generated, and laid out differently by PowerShell 5 and 7: compare its content.
                    $text = (New-Object IO.StreamReader $stream).ReadToEnd() | ConvertFrom-Json | ConvertTo-Json -Compress -Depth 10
                    "manifest.json $([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($text))))"
                } else {
                    "$($_.FullName) $([BitConverter]::ToString($sha.ComputeHash($stream)))"
                }
            } finally {
                $stream.Dispose()
            }
        }
    } finally {
        $zip.Dispose()
    }
}
$changed = Compare-Object @(Contents $xpi) @(Contents $signed)
if ($changed) {
    $changed | ForEach-Object { "  $($_.InputObject.Split(' ')[0])" } | Sort-Object -Unique
    throw "The extension changed (files above) but version $version is already signed. Raise `"version`" in extension\manifest.json."
}

# What installed copies check for updates: this version, attached to the app's
# GitHub release (every release carries it, changed or not).
$appVersion = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$hash = (Get-FileHash $signed -Algorithm SHA256).Hash.ToLower()
$updates = @{
    addons = @{
        $addonId = @{
            updates = @(@{
                version = $version
                update_link = "https://github.com/ahmedanjum30/BDownloader/releases/download/v$appVersion/bdownloader-firefox-$version.xpi"
                update_hash = "sha256:$hash"
            })
        }
    }
}
$json = $updates | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText((Join-Path $dist 'firefox-updates.json'), $json, (New-Object Text.UTF8Encoding $false))
"Wrote $(Join-Path $dist 'firefox-updates.json')"
