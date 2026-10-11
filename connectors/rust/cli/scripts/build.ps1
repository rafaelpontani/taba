<#
.SYNOPSIS
  Constrói o taba-cli no Windows (doc/19b-cli.md, seção 18.11.8).

.DESCRIPTION
  Sem argumento, constrói para a plataforma desta máquina. -Target constrói um alvo da tabela e
  -All constrói todos os que a máquina consegue (x86_64-pc-windows-msvc e aarch64-pc-windows-msvc;
  os do Linux e do macOS ficam em build.sh). O resultado vai para connectors\cli\dist\<alvo>\, com
  o binário e um SHA256SUMS.

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
$root = (Resolve-Path (Join-Path $here '..\..')).Path   # connectors\, o workspace do Cargo dos conectores
$dist = Join-Path (Split-Path $here -Parent) 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null

# Binário pequeno e reprodutível: o Cargo.lock vale (--locked), sem símbolos, otimizado para tamanho.
$env:CARGO_PROFILE_RELEASE_OPT_LEVEL = 'z'
$env:CARGO_PROFILE_RELEASE_LTO = 'true'
$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '1'
$env:CARGO_PROFILE_RELEASE_PANIC = 'abort'
$env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'

$hostTriple = ((rustc -vV) | Where-Object { $_ -like 'host:*' }) -replace '^host:\s*', ''
if ($hostTriple -notlike '*-windows-msvc') {
  Write-Error "build.ps1: sistema não suportado ($hostTriple); no Linux e no macOS use build.sh"
  exit 2
}
$reachable = @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')

function Build-One([string]$triple) {
  Write-Host "==> $triple"
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'   # o rustup escreve avisos informativos na saída de erro
  rustup target add $triple 2>&1 | Out-Null
  $ErrorActionPreference = $previous
  Push-Location $root
  try {
    cargo build --release --locked -p taba-cli --target $triple
    if ($LASTEXITCODE -ne 0) { throw "cargo build falhou para $triple" }
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
    Write-Error "build.ps1: esta máquina ($hostTriple) não constrói $Target (alcança: $($reachable -join ', '))"
    exit 2
  }
  Build-One $Target
} else {
  Build-One ($hostTriple)
}
