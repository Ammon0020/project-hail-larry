<#
.SYNOPSIS
Verifies and installs prerequisites for building the Local Agent Interface.

.DESCRIPTION
Checks Node.js/npm, the Rust toolchain, and frontend deps (web\node_modules).
Run with -Verify to only check (used by build.ps1); run plain to also attempt
auto-fixable installs (e.g. `npm ci` in web/).
#>

param(
    [switch]$Verify
)

$ErrorActionPreference = "Stop"

# Resolve repo root as the parent of this script's directory (scripts\).
$RootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

# --- Helpers -----------------------------------------------------------------

# Compare two version strings ("1.92.0" vs "1.92.0") as [version]. Pads to
# major.minor since [version] requires at least two components.
function Test-VersionGe {
    param(
        [string]$Actual,
        [string]$Required
    )
    function ConvertTo-Version([string]$v) {
        $parts = $v.Trim().Split('.')
        # Pad to at least major.minor.build so [version] casts cleanly.
        while ($parts.Count -lt 3) { $parts += '0' }
        return [version]($parts -join '.')
    }
    $a = ConvertTo-Version $Actual
    $r = ConvertTo-Version $Required
    return ($a -ge $r)
}

# Read the pinned Rust channel from rust-toolchain.toml, defaulting to 1.92.0.
function Get-RustChannel {
    $toml = Join-Path $RootDir 'rust-toolchain.toml'
    if (Test-Path $toml) {
        foreach ($line in Get-Content $toml) {
            if ($line -match 'channel\s*=\s*"([^"]+)"') {
                return $Matches[1]
            }
        }
    }
    return '1.92.0'
}

# Print a red, multi-line ERROR block. Uses $host.UI.WriteErrorLine so the
# message lands on the error stream (visible in CI/logs/piped contexts) and
# is colored red in an interactive console. Unlike Write-Host, this is
# captured by stream redirection; unlike Write-Error, it does not produce a
# record that throws under $ErrorActionPreference = "Stop".
function Write-Failure {
    param([string]$Message)
    $host.UI.WriteErrorLine($Message)
}

# --- Checks ------------------------------------------------------------------

$failures = @()

# node
$nodeOk = $false
$nodeVersion = $null
if (Get-Command node -ErrorAction SilentlyContinue) {
    $raw = (& node --version).Trim()  # e.g. "v20.11.0"
    $nodeVersion = $raw -replace '^v', ''
    if (-not (Test-VersionGe $nodeVersion '20.0.0')) {
        $failures += @"
ERROR: node version $nodeVersion is older than the required 20.
  Upgrade: https://nodejs.org/ or ``nvm install 20``
"@
    } else {
        $nodeOk = $true
    }
} else {
    $failures += @"
ERROR: 'node' is not installed or not on PATH.
  Required: Node.js 20 or newer.
  Install: https://nodejs.org/ or via nvm-windows (https://github.com/coreybutler/nvm-windows)
"@
}

# npm
$npmOk = $false
if (Get-Command npm -ErrorAction SilentlyContinue) {
    $npmOk = $true
} else {
    $failures += @"
ERROR: 'npm' is not installed or not on PATH.
  Required: npm (ships with Node.js).
  Install: https://nodejs.org/ or via nvm-windows (https://github.com/coreybutler/nvm-windows)
"@
}

# cargo
$cargoOk = $false
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $cargoOk = $true
} else {
    $failures += @"
ERROR: 'cargo' is not installed or not on PATH.
  Required: Rust toolchain.
  Install: https://rustup.rs/
"@
}

# rustc (version pinned in rust-toolchain.toml)
$rustcOk = $false
$requiredRust = Get-RustChannel
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    $raw = (& rustc --version).Trim()  # e.g. "rustc 1.92.0 (...)"
    # Extract the version token after "rustc ".
    if ($raw -match 'rustc\s+(\d+\.\d+\.\d+)') {
        $rustcVersion = $Matches[1]
        if (-not (Test-VersionGe $rustcVersion $requiredRust)) {
            $failures += @"
ERROR: rustc version $rustcVersion is older than the required $requiredRust.
  Upgrade: ``rustup update`` or https://rustup.rs/
"@
        } else {
            $rustcOk = $true
        }
    } else {
        # Couldn't parse; treat as a failure with a generic message.
        $failures += @"
ERROR: could not parse rustc version from '$raw'.
  Required: rustc $requiredRust or newer.
  Install/Upgrade: https://rustup.rs/
"@
    }
} else {
    $failures += @"
ERROR: 'rustc' is not installed or not on PATH.
  Required: rustc $requiredRust or newer.
  Install: https://rustup.rs/
"@
}

# sccache
if (-not (Get-Command sccache -ErrorAction SilentlyContinue)) {
    $failures += @"
ERROR: 'sccache' is not installed or not on PATH.
  Required: sccache for build and test caching.
  Install: ``cargo install sccache`` or via package manager (e.g. ``winget install Mozilla.sccache`` / ``choco install sccache``)
"@
}

# web\node_modules
$nodeModulesPath = Join-Path $RootDir 'web\node_modules'
$nodeModulesOk = Test-Path $nodeModulesPath
if (-not $nodeModulesOk) {
    $failures += @"
ERROR: web\node_modules is missing — frontend dependencies are not installed.
  Run: .\scripts\setup.ps1   (or: cd web; npm ci)
"@
}

# --- Act on results ----------------------------------------------------------

if ($failures.Count -gt 0) {
    foreach ($f in $failures) {
        Write-Failure $f
    }

    # In install mode, attempt the only auto-fixable step: npm ci for deps.
    # Only safe to run when node and npm are present; other failures (missing
    # cargo/rustc, outdated versions) cannot be auto-fixed here.
    if (-not $Verify -and $nodeOk -and $npmOk -and -not $nodeModulesOk) {
        Write-Host "Installing frontend dependencies (npm ci)..." -ForegroundColor Cyan
        Push-Location (Join-Path $RootDir 'web')
        try {
            npm ci
        } finally {
            Pop-Location
        }
        # Re-check after install; if it failed, report and exit.
        if (-not (Test-Path $nodeModulesPath)) {
            Write-Failure "ERROR: npm ci did not produce web\node_modules."
            exit 1
        }
        # node_modules is now fixed. If that was the only failure, succeed;
        # otherwise other unfixable failures remain (already printed above).
        if ($failures.Count -eq 1) {
            Write-Host "Setup complete!" -ForegroundColor Green
            exit 0
        }
    }

    # Verify mode, or install mode with unfixable failures remaining.
    exit 1
}

# All prerequisites satisfied.
if ($Verify) {
    Write-Host "Prerequisites OK" -ForegroundColor Green
} else {
    Write-Host "Setup complete!" -ForegroundColor Green
}
exit 0
