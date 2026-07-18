# SPA smoke test for a Windows release (or debug) local_agent.exe.
#
# Starts the daemon with an isolated LOCAL_AGENT_STATE_DIR, probes /health and
# / (HTML), then stops cleanly. Never touches ~/.local-agent.
#
# Usage:
#   pwsh scripts/spa-smoke.ps1 [-Binary path\to\local_agent.exe]

param(
    [string]$Binary = ""
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

if (-not $Binary) {
    $candidate = Join-Path $Root "target\release\local_agent.exe"
    if (Test-Path $candidate) {
        $Binary = $candidate
    } else {
        throw "ERROR: no -Binary given and target\release\local_agent.exe missing"
    }
}

if (-not (Test-Path $Binary)) {
    throw "ERROR: binary not found: $Binary"
}

# Free TCP port on loopback.
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$Port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()

$StateDir = Join-Path ([System.IO.Path]::GetTempPath()) ("local-agent-spa-smoke-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
$env:LOCAL_AGENT_STATE_DIR = $StateDir

function Stop-SmokeDaemon {
    try {
        $env:LOCAL_AGENT_STATE_DIR = $StateDir
        & $Binary stop 2>$null | Out-Null
    } catch {
        # Best-effort cleanup.
    }
}

try {
    # camelCase keys match Config serde (rename_all = "camelCase").
    # TOML needs doubled backslashes in Windows path strings. Avoid UTF-8 BOM
    # (Set-Content -Encoding utf8 on Windows PowerShell writes a BOM).
    $stateToml = $StateDir.Replace('\', '\\')
    $dbToml = (Join-Path $StateDir "local-agent.db").Replace('\', '\\')
    $config = @"
port = $Port
host = "127.0.0.1"
dataDir = "$stateToml"
dbPath = "$dbToml"
tlsEnabled = false
pairingTtlSeconds = 300
"@
    [System.IO.File]::WriteAllText((Join-Path $StateDir "config.toml"), $config)

    Write-Host "[spa-smoke] binary=$Binary"
    Write-Host "[spa-smoke] state_dir=$StateDir"
    Write-Host "[spa-smoke] port=$Port"

    & $Binary start --background
    if ($LASTEXITCODE -ne 0) {
        throw "ERROR: start --background failed with exit $LASTEXITCODE"
    }

    $base = "http://127.0.0.1:$Port"
    $ready = $false
    for ($i = 0; $i -lt 60; $i++) {
        try {
            $resp = Invoke-WebRequest -Uri "$base/health" -UseBasicParsing -TimeoutSec 2
            if ($resp.StatusCode -eq 200) {
                $ready = $true
                break
            }
        } catch {
            Start-Sleep -Milliseconds 500
        }
    }

    if (-not $ready) {
        Write-Host "ERROR: daemon did not become ready at $base/health within 30s" -ForegroundColor Red
        try { & $Binary logs } catch {}
        throw "daemon not ready"
    }

    $health = (Invoke-WebRequest -Uri "$base/health" -UseBasicParsing -TimeoutSec 5).Content
    Write-Host "[spa-smoke] /health => $health"

    $body = (Invoke-WebRequest -Uri "$base/" -UseBasicParsing -TimeoutSec 5).Content
    if ($body -notmatch '(?i)<!doctype html|Local Agent') {
        throw "ERROR: / did not look like embedded SPA HTML (first 400 chars): $($body.Substring(0, [Math]::Min(400, $body.Length)))"
    }
    Write-Host "[spa-smoke] / returned HTML (SPA embed OK)"

    & $Binary stop
    if ($LASTEXITCODE -ne 0) {
        throw "ERROR: stop failed with exit $LASTEXITCODE"
    }

    Write-Host "[spa-smoke] OK"
}
finally {
    Stop-SmokeDaemon
    if (Test-Path $StateDir) {
        Remove-Item -Recurse -Force $StateDir -ErrorAction SilentlyContinue
    }
}
