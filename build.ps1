<#
.SYNOPSIS
Builds the Local Agent Interface project (Frontend + Go + Rust backends).

.DESCRIPTION
This script builds the React frontend, copies the compiled assets into the
Go server's embed directory, builds the Go executable, and then builds the
Rust port (which embeds web/dist via rust-embed at compile time).
#>

$ErrorActionPreference = "Stop"

# Fail loudly if required tooling is missing.
foreach ($tool in @("npm", "go", "cargo")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Write-Host "ERROR: '$tool' is not installed or not on PATH." -ForegroundColor Red
        exit 1
    }
}

Write-Host "1. Building frontend..." -ForegroundColor Cyan
Set-Location "web"
npm run build
Set-Location ".."

Write-Host "2. Copying frontend assets to Go embed directory..." -ForegroundColor Cyan
$distDir = "internal\server\dist"
if (Test-Path $distDir) {
    Remove-Item -Recurse -Force "$distDir\*"
} else {
    New-Item -ItemType Directory -Force -Path $distDir | Out-Null
}
Copy-Item -Recurse -Force "web\dist\*" "$distDir\"

Write-Host "3. Building Go backend..." -ForegroundColor Cyan
# Build into the bin folder
go build -o bin\app.exe .\cmd\app
# Also install it to GOPATH so 'app start' works globally
go install .\cmd\app

Write-Host "4. Building Rust backend..." -ForegroundColor Cyan
# rust-embed bakes web/dist into the binary at compile time, so the frontend
# build above is picked up automatically -- no copy step needed. The Rust port
# outputs a separate binary (bin\local_agent.exe) alongside the Go binary.
cargo build --release
Copy-Item -Force "target\release\local_agent.exe" "bin\local_agent.exe"

Write-Host "Build complete!" -ForegroundColor Green
Write-Host "  Go binary:   bin\app.exe          (installed globally as 'app')" -ForegroundColor Green
Write-Host "  Rust binary: bin\local_agent.exe  (run with 'bin\local_agent.exe --serve')" -ForegroundColor Green
