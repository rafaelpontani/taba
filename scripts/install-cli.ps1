<#
.SYNOPSIS
  Installs the taba-cli (the Taba command line) on Windows.

.DESCRIPTION
  The GitHub Releases carry only the Linux binaries, so on Windows there are two ways:
    1. cargo (default when it exists): `cargo install taba-cli --locked` (needs the Rust toolchain with the MSVC build tools).
    2. Docker (when there is no cargo, or with -Docker): creates a `taba-cli.cmd` that runs the image
       rafaelpontani/taba-cli. Inside the container, 127.0.0.1 is not your machine, so the shim points
       TABA_CLI_URL to http://host.docker.internal:8080 unless you set it.

  powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/rafaelpontani/taba/main/scripts/install-cli.ps1 | iex"

.PARAMETER Version
  Version to install, for example 0.1.1 (default: the latest).

.PARAMETER Docker
  Forces the Docker shim even when cargo exists.

.PARAMETER Dir
  Folder of the Docker shim (default: $HOME\.taba\bin).
#>
param(
    [string]$Version = $env:TABA_CLI_VERSION,
    [switch]$Docker,
    [string]$Dir = $(if ($env:TABA_CLI_DIR) { $env:TABA_CLI_DIR } else { Join-Path $HOME '.taba\bin' })
)

$ErrorActionPreference = 'Stop'

function Fail($message) { Write-Error "install-cli: $message"; exit 1 }

$hasCargo = [bool](Get-Command cargo -ErrorAction SilentlyContinue)
$hasDocker = [bool](Get-Command docker -ErrorAction SilentlyContinue)

if ($hasCargo -and -not $Docker) {
    $cargoArgs = @('install', 'taba-cli', '--locked')
    if ($Version) { $cargoArgs += @('--version', $Version.TrimStart('v')) }
    Write-Host "cargo $($cargoArgs -join ' ')"
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { Fail 'cargo install failed (is the MSVC build tools installed?)' }
    Write-Host "Installed: $HOME\.cargo\bin\taba-cli.exe"
    & taba-cli --version
    exit 0
}

if (-not $hasDocker) {
    Fail 'neither cargo (https://rustup.rs) nor Docker was found'
}

$tag = if ($Version) { $Version.TrimStart('v') } else { 'latest' }
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
$shim = Join-Path $Dir 'taba-cli.cmd'
$lines = @(
    '@echo off',
    'if "%TABA_CLI_URL%"=="" set "TABA_CLI_URL=http://host.docker.internal:8080"',
    "docker run --rm -i -e TABA_CLI_URL -e TABA_CLI_TOKEN rafaelpontani/taba-cli:$tag %*"
)
Set-Content -Path $shim -Value $lines -Encoding ASCII
Write-Host "Installed: $shim (runs rafaelpontani/taba-cli:$tag in Docker)"

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $Dir) {
    Write-Host "Note: $Dir is not in your PATH. To add it:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"`$env:Path;$Dir`", 'User')"
}
