<#
.SYNOPSIS
Builds the Local Agent Interface (frontend + Rust daemon).

.DESCRIPTION
Primary binary: bin\local_agent.exe.
#>

$ErrorActionPreference = "Stop"

foreach ($tool in @("npm", "cargo")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Write-Host "ERROR: '$tool' is not installed or not on PATH." -ForegroundColor Red
        exit 1
    }
}

Write-Host "1. Building frontend..." -ForegroundColor Cyan
Set-Location "web"
npm run build
Set-Location ".."

Write-Host "2. Building Rust daemon (local_agent)..." -ForegroundColor Cyan
cargo build --release
New-Item -ItemType Directory -Force -Path "bin" | Out-Null
Copy-Item -Force "target\release\local_agent.exe" "bin\local_agent.exe"

$cargoBin = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME "bin" } else { Join-Path $HOME ".cargo\bin" }
if (Test-Path $cargoBin) {
    Copy-Item -Force "bin\local_agent.exe" (Join-Path $cargoBin "local_agent.exe")
    Write-Host "  Installed: $cargoBin\local_agent.exe"
}

Write-Host "Build complete!" -ForegroundColor Green
Write-Host "  Rust daemon: bin\local_agent.exe   (run: local_agent start)"
