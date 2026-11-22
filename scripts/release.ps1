<#
.SYNOPSIS
  Releases B-Downloader: tags main with the next version and pushes the tag.
  No version-bump commit: the tag is the version, and the Release workflow
  writes it into Cargo.toml (scripts/set-version.sh) before building.

  The release notes are that version's section of CHANGELOG.md. If it has
  none yet but [Unreleased] has entries, they become the new version's
  section, in a commit that goes up with the tag.

.EXAMPLE
  scripts\release.ps1            # 1.5.3 -> 1.5.4
  scripts\release.ps1 minor      # 1.5.3 -> 1.6.0
  scripts\release.ps1 major      # 1.5.3 -> 2.0.0
  scripts\release.ps1 1.6.0      # exact version
  scripts\release.ps1 -DryRun    # show the next version and stop
#>
param(
    [string]$Bump = 'patch',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Invoke-Checked([string]$what, [scriptblock]$cmd) {
    & $cmd
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit code $LASTEXITCODE)." }
}

# --- Checks -----------------------------------------------------------------
$branch = (git rev-parse --abbrev-ref HEAD).Trim()
if ($branch -ne 'main') { throw "Releases go out from main; you're on '$branch'." }
if (git status --porcelain) { throw 'There are uncommitted changes. Commit them (or stash them) first: a release is what is committed.' }
Invoke-Checked 'git pull' { git pull -q --ff-only origin main }
Invoke-Checked 'git fetch --tags' { git fetch -q --tags origin }

# The current version is the newest release tag.
$latest = git tag --list 'v*' | Where-Object { $_ -match '^v\d+\.\d+\.\d+$' } |
    Sort-Object { [version]($_.TrimStart('v')) } | Select-Object -Last 1
if (-not $latest) { throw 'No release tags (vX.Y.Z) found.' }
$current = $latest.TrimStart('v')
$major, $minor, $patch = $current.Split('.') | ForEach-Object { [int]$_ }

switch -Regex ($Bump) {
    '^patch$' { $next = "$major.$minor.$($patch + 1)" }
    '^minor$' { $next = "$major.$($minor + 1).0" }
    '^major$' { $next = "$($major + 1).0.0" }
    '^v?\d+\.\d+\.\d+$' { $next = $Bump.TrimStart('v') }
    default { throw "Use patch, minor, major or a version like 1.6.0 (got '$Bump')." }
}
if ([version]$next -le [version]$current) { throw "$next isn't newer than the current $current." }
if (git tag --list "v$next") { throw "Tag v$next already exists." }

# The release notes come from CHANGELOG.md.
$changelog = [IO.File]::ReadAllText((Join-Path $root 'CHANGELOG.md'))
$hasSection = $changelog.Contains("## [$next]")
$unreleased = [regex]::Match($changelog, '(?s)## \[Unreleased\]\r?\n(.*?)(?=\r?\n## \[)').Groups[1].Value
if (-not $hasSection -and -not $unreleased.Trim()) {
    throw "CHANGELOG.md has nothing for ${next}: add what changed under [Unreleased] (or a '## [$next]' section) first."
}

Write-Host "Releasing $current -> $next (main at $((git rev-parse --short HEAD).Trim()))"
if (-not $hasSection) { Write-Host "  [Unreleased] becomes [$next] in CHANGELOG.md" }
if ($DryRun) { return }

# --- Changelog --------------------------------------------------------------
if (-not $hasSection) {
    $date = (Get-Date).ToString('yyyy-MM-dd')
    $updated = $changelog.Replace("## [Unreleased]`n", "## [Unreleased]`n`n## [$next] - $date`n")
    [IO.File]::WriteAllText((Join-Path $root 'CHANGELOG.md'), $updated, (New-Object Text.UTF8Encoding $false))
    $message = Join-Path ([IO.Path]::GetTempPath()) 'bdl-release-message.txt'
    [IO.File]::WriteAllText($message, "Changelog for $next`n", (New-Object Text.UTF8Encoding $false))
    Invoke-Checked 'git commit' { git commit -q -F $message -- CHANGELOG.md }
    Remove-Item $message
}

# --- Tag and push -----------------------------------------------------------
# Local commits not on GitHub yet go up first, so the tag points at something CI built.
if ((git rev-list --count origin/main..HEAD).Trim() -ne '0') { Invoke-Checked 'git push main' { git push -q origin main } }
Invoke-Checked 'git tag' { git tag "v$next" }
Invoke-Checked 'git push tag' { git push -q origin "v$next" }

Write-Host "Pushed v$next. The release builds on GitHub in about 10 minutes:"
Write-Host "  https://github.com/ahmedanjum30/B-Downloader/releases/tag/v$next"
