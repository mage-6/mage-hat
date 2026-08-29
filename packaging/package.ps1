# Builds the release binary and assembles the folder we hand to people.
#
#   pwsh packaging\package.ps1                                     this machine
#   pwsh packaging\package.ps1 -Target x86_64-unknown-linux-musl   a Rust target
#
# MANUAL.md is the documentation, and everything here is made from it: the
# binary embeds it and prints it for `magehat -h`, the package ships it as
# README.md, and SKILL.md is the same text under the agent-skill frontmatter.
# Nothing in the package is written by hand, so the three cannot drift apart.
#
# Cargo.toml is the only place the version lives. The script reads it there
# and refuses to package a binary that reports a different one, so a stale
# target/ can never ship under a fresh version number. The release workflow
# passes the git tag as -Tag, which must be v<version> for the same reason.

param(
    # Rust target triple. Omitted means this machine.
    [string]$Target,
    # The git tag being released; must equal v<version> from Cargo.toml.
    [string]$Tag
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$cargo = Get-Content (Join-Path $repo 'Cargo.toml') -Raw
if ($cargo -notmatch '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"') {
    throw "No version in Cargo.toml [package]"
}
$version = $Matches[1]
if ($Tag -and $Tag -ne "v$version") {
    throw "Tag '$Tag' does not match Cargo.toml version $version"
}

$hostOs = if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) { 'windows' }
          elseif ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::OSX)) { 'macos' }
          else { 'linux' }
$os = if (-not $Target) { $hostOs }
      elseif ($Target -like '*windows*') { 'windows' }
      elseif ($Target -like '*linux*') { 'linux' }
      elseif ($Target -like '*darwin*') { 'macos' }
      else { throw "Cannot tell the OS from target $Target" }
$arch = if ($Target -like 'aarch64*') { 'arm64' } else { 'x64' }
$exeName = if ($os -eq 'windows') { 'magehat.exe' } else { 'magehat' }

Write-Host "Building magehat $version for $os-$arch ..."
Push-Location $repo
try {
    $buildArgs = @('build', '--release')
    if ($Target) { $buildArgs += @('--target', $Target) }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
} finally {
    Pop-Location
}

$exe = if ($Target) { Join-Path $repo "target/$Target/release/$exeName" } else { Join-Path $repo "target/release/$exeName" }
if (-not (Test-Path $exe)) { throw "Missing $exe" }

# A binary built for another machine cannot be run here; everything else is checked.
if ($os -eq $hostOs -and $arch -eq 'x64') {
    $reported = (& $exe --version).Trim()
    if ($reported -ne "magehat $version") {
        throw "Binary reports '$reported' but Cargo.toml says $version"
    }
    # The binary must be able to explain itself on its own; that is the whole
    # point of embedding the manual, so a package where it cannot is a failure.
    $help = (& $exe -h) -join "`n"
    if ($help -notmatch [regex]::Escape("# MageHat $version")) {
        throw "magehat -h did not print the manual for $version"
    }
} else {
    Write-Host "Cross-built for $os-$arch; skipping the run checks"
}

$name = "magehat-$version-$os-$arch"
$releaseDir = Join-Path $repo 'release'
$stage = Join-Path $releaseDir $name
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage -Force | Out-Null

Copy-Item $exe (Join-Path $stage $exeName)
if ($os -ne 'windows' -and $hostOs -ne 'windows') { & chmod +x (Join-Path $stage $exeName) }

$manual = (Get-Content (Join-Path $repo 'MANUAL.md') -Raw) -replace '\{\{VERSION\}\}', $version
$header = Get-Content (Join-Path $PSScriptRoot 'skill-header.md') -Raw
Set-Content -Path (Join-Path $stage 'README.md') -Value $manual -Encoding utf8 -NoNewline
Set-Content -Path (Join-Path $stage 'SKILL.md') -Value ($header + $manual) -Encoding utf8 -NoNewline

if ($os -eq 'windows') {
    $archive = Join-Path $releaseDir "$name.zip"
    if (Test-Path $archive) { Remove-Item $archive -Force }
    Compress-Archive -Path $stage -DestinationPath $archive
} else {
    # tar keeps the executable bit, which a zip made here would lose.
    $archive = Join-Path $releaseDir "$name.tar.gz"
    if (Test-Path $archive) { Remove-Item $archive -Force }
    & tar -czf $archive -C $releaseDir $name
    if ($LASTEXITCODE -ne 0) { throw "tar failed" }
}

$size = [math]::Round((Get-Item $exe).Length / 1MB, 1)
$hash = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLower()

Write-Host ""
Write-Host "Packaged $name"
Write-Host "  folder   $stage"
Write-Host "  archive  $archive"
Write-Host "  exe      $size MB"
Write-Host "  sha256   $hash"
