# Local validation suite: QUICK (default) and FULL from any PowerShell directory.
#
# Usage:
#   .\scripts\check-local.ps1            # QUICK: fmt, clippy, Rust unit tests,
#                                        # web lint/build/test, git diff check.
#   .\scripts\check-local.ps1 QUICK
#   .\scripts\check-local.ps1 FULL       # QUICK + 34 Rust integration tests
#                                        # with disposable PostgreSQL & Mosquitto,
#                                        # Compose config check, Rust image build,
#                                        # and simulator ruff/pytest.
#
# Rust checks run inside `rust:1-slim-bookworm` with cached registry/rustup volumes.
# Simulator checks run inside `python:3.12-slim` with a cached pip volume.
# Web checks run on the host Node.js (reusing node_modules).
# Never touches the active laboratory containers or volumes on port 8083.

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('QUICK', 'FULL')]
    [string]$Mode = 'QUICK'
)

$ErrorActionPreference = 'Stop'
$repo = (Split-Path -Parent $PSScriptRoot) -replace '\\', '/'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$evidence = Join-Path $repo ".local-validation/check-$Mode-$stamp"
New-Item -ItemType Directory -Force -Path $evidence | Out-Null
$script:Results = New-Object System.Collections.Generic.List[object]
$script:Started = Get-Date

function Run {
    param([string]$Exe, [string[]]$ArgList, [string]$WorkingDirectory)
    $previous = $PWD
    try {
        if ($WorkingDirectory) { Set-Location $WorkingDirectory }
        & $Exe @ArgList
        if ($LASTEXITCODE -ne 0) {
            throw ("$Exe $($ArgList -join ' ') -> exit $LASTEXITCODE")
        }
    } finally { Set-Location $previous }
}

function Add-Result {
    param([string]$Name, [string]$Status, [double]$Seconds, [string]$Note)
    $script:Results.Add([pscustomobject]@{
        Step = $Name; Status = $Status
        Seconds = [math]::Round($Seconds, 1); Note = $Note })
}

function Invoke-Step {
    param([string]$Name, [scriptblock]$Body, [string]$SkipReason)
    if ($SkipReason) {
        Add-Result $Name 'SKIPPED' 0 $SkipReason
        Write-Host "SKIPPED: $Name - $SkipReason" -ForegroundColor DarkYellow
        return
    }
    $log = Join-Path $evidence "$Name.log"
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    Write-Host "`nRUN: $Name" -ForegroundColor Cyan
    try {
        $ErrorActionPreference = 'Continue'
        & $Body 2>&1 | Tee-Object -FilePath $log
        $ErrorActionPreference = 'Stop'
        $watch.Stop()
        Add-Result $Name 'PASS' $watch.Elapsed.TotalSeconds ''
        Write-Host ("PASS: {0} ({1:n1}s)" -f $Name, $watch.Elapsed.TotalSeconds) -ForegroundColor Green
    } catch {
        $ErrorActionPreference = 'Stop'
        $watch.Stop()
        Add-Result $Name 'FAIL' $watch.Elapsed.TotalSeconds "$_"
        Add-Content -Path $log -Value "`n[FAILED] $_"
        Write-Host ("FAIL: {0} - {1}" -f $Name, $_) -ForegroundColor Red
    }
}

function Invoke-Cargo {
    param([string[]]$CargoArgs, [string[]]$ExtraEnv = @(), [string]$Network = '')
    $dockerArgs = @('run', '--rm', '-v', "${repo}:/src", '-v',
        'locker-rustcheck-registry:/usr/local/cargo/registry', '-v',
        'locker-rustcheck-rustup-downloads:/usr/local/rustup/downloads',
        '-w', '/src/apps/api-rust')
    if ($Network) { $dockerArgs += @('--network', $Network) }
    foreach ($item in $ExtraEnv) { $dockerArgs += @('-e', $item) }
    $dockerArgs += @('rust:1-slim-bookworm', 'sh', '-c',
        "rustup component add rustfmt clippy >/dev/null 2>&1; cargo $($CargoArgs -join ' ')")
    Run docker $dockerArgs
}

function Wait-ContainerHealthy {
    param([string]$Description, [scriptblock]$Predicate, [int]$Seconds = 90)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Predicate) { return }
        Start-Sleep -Seconds 2
    }
    throw "timeout waiting for $Description"
}

& docker info *> $null
if ($LASTEXITCODE -ne 0) {
    throw 'Docker is not running. Start Docker Desktop before running check-local.ps1.'
}

# ----------------------------- QUICK ---------------------------------------
Invoke-Step 'rust-fmt' {
    Invoke-Cargo @('fmt', '--check')
}

Invoke-Step 'rust-clippy' {
    Invoke-Cargo @('clippy', '--locked', '--all-targets', '--', '-D', 'warnings')
}

Invoke-Step 'rust-tests-unit' {
    Invoke-Cargo @('test', '--locked')
}

$webDir = Join-Path $repo 'apps/web'
Invoke-Step 'web-deps' {
    $eslintBin = Join-Path $webDir 'node_modules/.bin/eslint.cmd'
    if (Test-Path $eslintBin) {
        Write-Output 'node_modules present with working .bin shims; npm ci skipped (dependencies reused)'
    } else {
        Run npm @('ci') $webDir
    }
}

Invoke-Step 'web-lint' {
    Run npm @('run', 'lint') $webDir
}

Invoke-Step 'web-build' {
    Run npm @('run', 'build') $webDir
}

Invoke-Step 'web-test' {
    Run npm @('test') $webDir
}

Invoke-Step 'git-diff-check' {
    Run git @('diff', '--check') $repo
    Run git @('diff', '--cached', '--check') $repo
}

# ----------------------------- FULL ----------------------------------------
if ($Mode -eq 'FULL') {
    Invoke-Step 'rust-tests-integration' {
        Run docker @('network', 'create', 'locker-check-net')
        try {
            Run docker @('run', '-d', '--name', 'locker-check-pg', '--network', 'locker-check-net',
                '-e', 'POSTGRES_USER=locker_rust', '-e', 'POSTGRES_PASSWORD=rust_test_only',
                '-e', 'POSTGRES_DB=locker_rust_test', 'postgres:17-alpine')
            Run docker @('run', '-d', '--name', 'locker-check-mqtt', '--network', 'locker-check-net',
                '-v', "${repo}/infra/docker/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro",
                'eclipse-mosquitto:2.0.22')
            Wait-ContainerHealthy 'integration PostgreSQL' {
                (docker exec locker-check-pg pg_isready -U locker_rust -d locker_rust_test *> $null); $LASTEXITCODE -eq 0
            } 90
            Wait-ContainerHealthy 'integration Mosquitto' {
                (docker exec locker-check-mqtt mosquitto_sub -h localhost -t '$SYS/broker/version' -C 1 -W 3 *> $null); $LASTEXITCODE -eq 0
            } 90
            Invoke-Cargo @('test', '--locked', '--', '--ignored') @(
                'LOCKER_TEST_DATABASE_URL=postgres://locker_rust:rust_test_only@locker-check-pg:5432/locker_rust_test',
                'LOCKER_TEST_MQTT_HOST=locker-check-mqtt') 'locker-check-net'
        } finally {
            docker rm -f locker-check-pg locker-check-mqtt *> $null
            docker network rm locker-check-net *> $null
        }
    }

    Invoke-Step 'compose-config' {
        Run docker @('compose', '-f', (Join-Path $repo 'compose.yaml'), 'config', '--quiet')
    }

    Invoke-Step 'rust-image-build' {
        Run docker @('compose', '-f', (Join-Path $repo 'compose.yaml'), 'build', 'api-rust', 'web', 'locker-simulator')
    }

    Invoke-Step 'python-simulator' {
        Run docker @('run', '--rm', '-v', "${repo}:/repo",
            '-v', 'locker-pipcheck-cache:/root/.cache/pip', '-w', '/repo/apps/locker-simulator',
            'python:3.12-slim', 'sh', '-c',
            "pip install -q -e '.[dev]' && ruff check . && pytest -q")
    }
}

# ------------------------------ Summary ------------------------------------
$total = ((Get-Date) - $script:Started).TotalSeconds
$failed = @($script:Results | Where-Object Status -eq 'FAIL')
$skipped = @($script:Results | Where-Object Status -eq 'SKIPPED')
Write-Host "`n===== check-local ($Mode) summary =====" -ForegroundColor Cyan
$script:Results | Format-Table Step, Status, Seconds, Note -AutoSize | Out-String -Width 200 | Write-Host
Write-Host ("steps: {0} PASS, {1} FAIL, {2} SKIPPED - total {3:n1}s" -f `
    @($script:Results | Where-Object Status -eq 'PASS').Count, $failed.Count, $skipped.Count, $total)
if ($failed.Count -gt 0) {
    Write-Host "evidence: $evidence"
    Write-Host 'check-local: FAILED' -ForegroundColor Red
    exit 1
}
Remove-Item -Recurse -Force $evidence -ErrorAction SilentlyContinue
Write-Host 'check-local: OK' -ForegroundColor Green
exit 0
