# Rust preview manager (F4.1): interactive start/stop of the isolated
# React + Rust stack on http://127.0.0.1:8084 without touching the Python
# laboratory (port 8083). Works from any PowerShell directory: the script
# resolves the repository from its own location.
#
# Usage:
#   .\rust-preview.ps1 start    # up; initializes schema + LAB seed only the
#                               # first time (never re-seeds an existing DB)
#   .\rust-preview.ps1 stop     # stops containers, KEEPS volumes and data
#   .\rust-preview.ps1 status   # containers + health of the preview
#   .\rust-preview.ps1 init     # explicit alembic + seed (refuses non-empty DB)
#   .\rust-preview.ps1 reset    # explicit teardown WITH volumes (destroys data)
#
# The CI smoke (scripts/test_rust_preview.py) keeps its own disposable
# lifecycle and is unaffected by this script.

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('start', 'stop', 'status', 'init', 'reset')]
    [string]$Action = 'status'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$composeFile = Join-Path $repo 'compose.rust-preview.yaml'
$base = 'http://127.0.0.1:8084'

if (-not (Test-Path $composeFile)) {
    throw "compose.rust-preview.yaml not found at $composeFile"
}

function Invoke-Compose {
    param([string[]]$ComposeArgs)
    & docker compose -p locker-rust-preview -f $composeFile @ComposeArgs
}

function Wait-Healthy {
    param([string]$Description, [scriptblock]$Predicate, [int]$Seconds = 120)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Predicate) { return }
        Start-Sleep -Seconds 2
    }
    throw "Timeout waiting for $Description"
}

function Test-Schema {
    $out = Invoke-Compose @('exec', '-T', 'postgres', 'psql', '-U', 'locker_rust', '-d', 'locker_rust', '-tAc', "SELECT to_regclass('public.locker_operations')")
    return [bool]($out -and $out.ToString().Trim())
}

function Initialize-Database {
    Write-Host 'Initializing schema and LAB seed (the seed refuses a non-empty database)...'
    Invoke-Compose @('run', '--rm', 'init', 'alembic', 'upgrade', 'head') | Out-Null
    Invoke-Compose @('run', '--rm', 'init', 'python', '-m', 'app.modules.seed') | Out-Null
    Write-Host 'Database initialized.'
}

# Docker must be running for anything below.
& docker info *> $null
if ($LASTEXITCODE -ne 0) {
    throw 'Docker is not running. Start Docker Desktop first.'
}

switch ($Action) {
    'start' {
        Invoke-Compose @('up', '-d', 'postgres', 'mosquitto') | Out-Null
        Wait-Healthy 'isolated PostgreSQL' { (Invoke-Compose @('exec', '-T', 'postgres', 'pg_isready', '-U', 'locker_rust', '-d', 'locker_rust') *> $null); $LASTEXITCODE -eq 0 }
        if (-not (Test-Schema)) {
            Initialize-Database
        } else {
            Write-Host 'Database already initialized; skipping seed (data preserved).'
        }
        Invoke-Compose @('up', '-d') | Out-Null
        Wait-Healthy 'preview readiness (Rust subscribed to MQTT events)' {
            try { (Invoke-WebRequest "$base/health/ready" -UseBasicParsing -TimeoutSec 5).StatusCode -eq 200 } catch { $false }
        }
        $live = try { (Invoke-WebRequest "$base/health/live" -UseBasicParsing -TimeoutSec 5).StatusCode } catch { 'unavailable' }
        Write-Host ''
        Write-Host "Rust preview running: $base  (health/live: $live)"
        Write-Host 'Laboratory credentials: admin / admin. Stop with: .\rust-preview.ps1 stop'
    }
    'stop' {
        Invoke-Compose @('stop') | Out-Null
        Write-Host 'Preview stopped. Volumes and data preserved (reset with: .\rust-preview.ps1 reset)'
    }
    'status' {
        Invoke-Compose @('ps')
        $ready = try { (Invoke-WebRequest "$base/health/ready" -UseBasicParsing -TimeoutSec 5).StatusCode } catch { 'unavailable' }
        Write-Host "health/ready via $base -> $ready"
    }
    'init' {
        Invoke-Compose @('up', '-d', 'postgres') | Out-Null
        Wait-Healthy 'isolated PostgreSQL' { (Invoke-Compose @('exec', '-T', 'postgres', 'pg_isready', '-U', 'locker_rust', '-d', 'locker_rust') *> $null); $LASTEXITCODE -eq 0 }
        Initialize-Database
    }
    'reset' {
        Write-Warning 'This removes the preview containers AND its volumes (data destroyed).'
        Invoke-Compose @('down', '-v') | Out-Null
        Write-Host 'Preview reset: containers and volumes removed.'
    }
}
