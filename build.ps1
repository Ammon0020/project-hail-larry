<#
.SYNOPSIS
Builds the Local Agent Interface project (Frontend + Backend).

.DESCRIPTION
This script builds the React frontend, copies the compiled assets into the
Go server's embed directory, and then builds the final Go executable.
#>

$ErrorActionPreference = "Stop"

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

Write-Host "Build complete! The binary is available at bin\app.exe and installed globally as 'app'." -ForegroundColor Green
