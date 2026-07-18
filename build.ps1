<#
.SYNOPSIS
Builds the Local Agent Interface (frontend + Rust daemon).

.DESCRIPTION
Primary binary: bin\local_agent.exe. Set $env:BUILD_GO=1 to also build the
legacy Go bin\app.exe.
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

if ($env:BUILD_GO -eq "1") {
    if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
        Write-Host "ERROR: BUILD_GO=1 but 'go' is not installed or not on PATH." -ForegroundColor Red
        exit 1
    }
    Write-Host "3. Building legacy Go daemon (BUILD_GO=1)..." -ForegroundColor Yellow
    $distDir = "internal\server\dist"
    if (Test-Path $distDir) {
        Remove-Item -Recurse -Force "$distDir\*"
    } else {
        New-Item -ItemType Directory -Force -Path $distDir | Out-Null
    }
    Copy-Item -Recurse -Force "web\dist\*" "$distDir\"
    go build -o bin\app.exe .\cmd\app
    go install .\cmd\app
    Write-Host "Build complete!" -ForegroundColor Green
    Write-Host "  Rust (default): bin\local_agent.exe"
    Write-Host "  Go (legacy):    bin\app.exe"
} else {
    Write-Host "Build complete!" -ForegroundColor Green
    Write-Host "  Rust daemon: bin\local_agent.exe   (run: local_agent start)"
    Write-Host "  Tip: `$env:BUILD_GO=1 also builds the legacy Go bin\app.exe"
}
