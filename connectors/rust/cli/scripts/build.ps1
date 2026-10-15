<#
.SYNOPSIS
  Builds taba-cli on Windows (doc/19b-cli.md, section 18.11.8).

.DESCRIPTION
  With no argument, builds for this machine's platform. -Target builds one target from the table and
  -All builds every one the machine can (x86_64-pc-windows-msvc and aarch64-pc-windows-msvc;
  the Linux and macOS ones live in build.sh). The result goes to connectors\cli\dist\<target>\, with
  the binary and a SHA256SUMS.

.EXAMPLE
  .\build.ps1
  .\build.ps1 -Target aarch64-pc-windows-msvc
  .\build.ps1 -All
#>
param(
  [string]$Target = '',
  [switch]$All
)

$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path $here '..\..')).Path   # connectors\, the Cargo workspace of the connectors
$dist = Join-Path (Split-Path $here -Parent) 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null

# Small, reproducible binary: Cargo.lock applies (--locked), no symbols, optimized for size.
$env:CARGO_PROFILE_RELEASE_OPT_LEVEL = 'z'
$env:CARGO_PROFILE_RELEASE_LTO = 'true'
$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '1'
$env:CARGO_PROFILE_RELEASE_PANIC = 'abort'
$env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'

$hostTriple = ((rustc -vV) | Where-Object { $_ -like 'host:*' }) -replace '^host:\s*', ''
if ($hostTriple -notlike '*-windows-msvc') {
  Write-Error "build.ps1: unsupported system ($hostTriple); on Linux and macOS use build.sh"
  exit 2
}
$reachable = @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')

function Build-One([string]$triple) {
  Write-Host "==> $triple"
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'   # rustup writes informational warnings to the error output
  rustup target add $triple 2>&1 | Out-Null
  $ErrorActionPreference = $previous
  Push-Location $root
  try {
    cargo build --release --locked -p taba-cli --target $triple
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $triple" }
  } finally {
    Pop-Location
  }
  $out = Join-Path $dist $triple
  New-Item -ItemType Directory -Force -Path $out | Out-Null
  Copy-Item (Join-Path $root "target\$triple\release\taba-cli.exe") (Join-Path $out 'taba-cli.exe') -Force
  $hash = (Get-FileHash (Join-Path $out 'taba-cli.exe') -Algorithm SHA256).Hash.ToLower()
  Set-Content -Path (Join-Path $out 'SHA256SUMS') -Value "$hash  taba-cli.exe" -Encoding ascii
  Write-Host "    $out\taba-cli.exe"
}

if ($All) {
  $failed = $false
  foreach ($t in $reachable) {
    try { Build-One $t } catch { Write-Warning $_; $failed = $true }
  }
  if ($failed) { exit 1 }
} elseif ($Target) {
  if ($reachable -notcontains $Target) {
    Write-Error "build.ps1: this machine ($hostTriple) does not build $Target (it reaches: $($reachable -join ', '))"
    exit 2
  }
  Build-One $Target
} else {
  Build-One ($hostTriple)
}
