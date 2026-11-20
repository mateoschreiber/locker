# Local validation (F4.1-D): QUICK / FULL / E2E from any PowerShell directory.
# This is the primary validation mechanism; GitHub Actions stays as an
# on-demand remote check (workflow_dispatch). The repository is resolved from
# this script's location, and the laboratory (8083), the interactive preview
# (8084, project locker-rust-preview) and their data are never modified:
# every disposable environment uses its own Docker project name and is
# cleaned up at the end, failures included.
#
# Usage:
#   .\scripts\check-local.ps1            # QUICK (default): fmt, clippy,
#                                        # Rust unit tests, web lint/build/test,
#                                        # staged+unstaged diff check.
#   .\scripts\check-local.ps1 QUICK
#   .\scripts\check-local.ps1 FULL       # QUICK + Rust integration with
#                                        # disposable PostgreSQL/Mosquitto,
#                                        # MQTT scenarios, Compose checks,
#                                        # image smoke, Python checks, parity
#                                        # and the Python restart regression.
#   .\scripts\check-local.ps1 E2E        # FULL + isolated guard preview with
#                                        # a persistence marker, the complete
#                                        # smoke (login, loans, presence,
#                                        # restarts, UNKNOWN/FAILED,
#                                        # reconciliation) and proof that the
#                                        # interactive preview, its data and
#                                        # the laboratory remain untouched.
#
# Rust checks run inside a Linux container (rust:1-slim-bookworm) with a
# persistent registry volume, so dependencies are downloaded once and no
# host toolchain is needed. Python checks run in python:3.12-slim with a
# persistent pip cache. Web checks use the host Node (node_modules is reused;
# npm ci only runs when it is missing). No step hides failures: every step
# reports PASS, FAIL or SKIPPED with its reason, and the exit code is
# non-zero when anything fails. Evidence is kept under .local-validation/.

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('QUICK', 'FULL', 'E2E')]
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
    # Executes a native command, streaming its output through the pipeline
    # (captured by the running step's Tee-Object) and throws on a non-zero
    # exit code so the step records FAIL instead of hiding the error.
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
        # Continue inside the step: with Stop, a native command writing to
        # stderr (clippy warnings, build chatter) under 2>&1 becomes a
        # terminating error and records false failures. Real failures come
        # from Run's explicit throw on a non-zero exit code.
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
    # Rust checks inside Linux (CI-equivalent). The registry and the rustup
    # download caches live in named volumes so dependencies and components
    # are not redownloaded on later runs; the build cache lives in the
    # gitignored apps/api-rust/target. fmt/clippy are components the slim
    # image ships without, so they are ensured (a cached no-op after the
    # first run) before every cargo invocation.
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
    param([string]$Description, [scriptblock]$Predicate, [int]$Seconds = 60)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Predicate) { return }
        Start-Sleep -Seconds 2
    }
    throw "timeout waiting for $Description"
}

$webDir = Join-Path $repo 'apps/web'
$nodeMissing = -not (Get-Command node -ErrorAction SilentlyContinue) -or
    -not (Get-Command npm -ErrorAction SilentlyContinue)
$nodeModules = Join-Path $webDir 'node_modules'

# --- Baselines of environments that must remain untouched (read-only). ------
function Get-HttpStatus {
    param([string]$Url)
    try { return (Invoke-WebRequest $Url -UseBasicParsing -TimeoutSec 5).StatusCode }
    catch { return 'unreachable' }
}
$script:LabBaseline = if ($Mode -eq 'E2E') { Get-HttpStatus 'http://127.0.0.1:8083/health/ready' } else { $null }
if ($Mode -eq 'E2E') {
    Write-Host "E2E: laboratory baseline (8083/health/ready) = $script:LabBaseline"
}

# ----------------------------- QUICK ---------------------------------------
Invoke-Step 'rust-fmt' { Invoke-Cargo @('fmt', '--check') }

Invoke-Step 'rust-clippy' { Invoke-Cargo @('clippy', '--locked', '--all-targets', '--', '-D', 'warnings') }

Invoke-Step 'rust-tests-unit' { Invoke-Cargo @('test', '--locked') }

Invoke-Step 'web-deps' {
    # Reuse only a WORKING install: the .cmd shims must exist (a
    # node_modules/ directory alone can be a broken partial install).
    if ((Test-Path "$nodeModules/.bin/eslint.cmd") -and
        (Test-Path "$nodeModules/.bin/tsc.cmd") -and
        (Test-Path "$nodeModules/.bin/vitest.cmd")) {
        Write-Output "node_modules present with working .bin shims; npm ci skipped (dependencies reused)"
    } else {
        Run npm @('ci') $webDir
    }
} -SkipReason:$(if ($nodeMissing) { 'node/npm not on PATH' } else { $null })

foreach ($webStep in @(
        @{ Name = 'web-lint'; Script = @('run', 'lint') },
        @{ Name = 'web-build'; Script = @('run', 'build') },
        @{ Name = 'web-test'; Script = @('test') })) {
    Invoke-Step $webStep.Name {
        Run npm $webStep.Script $webDir
    } -SkipReason:$(if ($nodeMissing) { 'node/npm not on PATH' } else { $null })
}

Invoke-Step 'git-diff-check' {
    # Both unstaged and staged content, exactly what a commit would contain.
    Run git @('diff', '--check')
    Run git @('diff', '--cached', '--check')
}

# ----------------------------- FULL ----------------------------------------
if ($Mode -in 'FULL', 'E2E') {

    Invoke-Step 'rust-tests-integration' {
        # Disposable PostgreSQL + Mosquitto on a private network, exactly
        # the CI shape; the ignored tests recreate their own schema.
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
            # Cleanup only; the original failure (if any) propagates.
            docker rm -f locker-check-pg locker-check-mqtt *> $null
            docker network rm locker-check-net *> $null
        }
    }

    Invoke-Step 'mqtt-scenarios' {
        Run python @('scripts/test_rust_mqtt.py') $repo
    }

    Invoke-Step 'compose-config' {
        Run docker @('compose', '-f', (Join-Path $repo 'compose.yaml'), 'config', '--quiet')
        $env:LOCKER_PREVIEW_PORT = '8085'
        try {
            Run docker @('compose', '-f', (Join-Path $repo 'compose.rust-preview.yaml'), 'config', '--quiet')
        } finally { Remove-Item env:LOCKER_PREVIEW_PORT -ErrorAction SilentlyContinue }
        Run docker @('compose', '-f', (Join-Path $repo 'compose.test.yaml'), 'config', '--quiet')
    }

    Invoke-Step 'rust-image-smoke' {
        Run docker @('compose', '-f', (Join-Path $repo 'compose.yaml'), '-p',
            'locker-local-check', '--profile', 'rust', 'build', 'api-rust')
        Run python @('scripts/test_rust_docker.py', 'locker-local-check-api-rust') $repo
    }

    Invoke-Step 'python-api' {
        Run docker @('run', '--rm', '-v', "${repo}:/repo",
            '-v', 'locker-pipcheck-cache:/root/.cache/pip', '-w', '/repo/apps/api',
            'python:3.12-slim', 'sh', '-c',
            "pip install -q -e '.[dev]' && ruff check . && mypy app && pytest -q")
    }

    Invoke-Step 'python-simulator' {
        Run docker @('run', '--rm', '-v', "${repo}:/repo",
            '-v', 'locker-pipcheck-cache:/root/.cache/pip', '-w', '/repo/apps/locker-simulator',
            'python:3.12-slim', 'sh', '-c',
            "pip install -q -e '.[dev]' && ruff check . && pytest -q")
    }

    Invoke-Step 'parity-python-rust' {
        Run python @('scripts/test_phase2_parity.py') $repo
    }

    Invoke-Step 'python-restart-regression' {
        Run python @('scripts/test_docker_restart.py') $repo
    }
}

# ------------------------------ E2E ----------------------------------------
if ($Mode -eq 'E2E') {
    $guardProject = 'locker-rust-preview-guard'
    $guardCompose = @('-p', $guardProject, '-f', (Join-Path $repo 'compose.rust-preview.yaml'))
    # One single-argument SQL string: PowerShell array literals split
    # "+"-concatenated strings into separate arguments, which psql then
    # ignores ("extra command-line argument") leaving a vacuous invariant.
    $script:GuardCountsSql = "SELECT (SELECT count(*) FROM branches) || '/' || (SELECT count(*) FROM tools) || '/' || (SELECT count(*) FROM users) || '/' || (SELECT count(*) FROM loans) || '/' || (SELECT count(*) FROM locker_operations) || '/' || (SELECT count(*) FROM e2e_marker)"
    $script:GuardCounts = $null

    Invoke-Step 'e2e-guard-setup' {
        # A THIRD disposable preview-like environment (port 8086): proves the
        # smoke cannot touch a persistent preview's data. It is seeded, gets a
        # marker record and is destroyed at the end — the user's interactive
        # preview on 8084 is only ever observed read-only.
        $env:LOCKER_PREVIEW_PORT = '8086'
        $markerSql = "CREATE TABLE e2e_marker (id int primary key, note text); INSERT INTO e2e_marker VALUES (1, 'smoke isolation guard');"
        try {
            Run docker (@('compose') + $guardCompose + @('up', '-d', 'postgres', 'mosquitto'))
            Wait-ContainerHealthy 'guard PostgreSQL' {
                (docker compose @guardCompose exec -T postgres pg_isready -U locker_rust -d locker_rust *> $null); $LASTEXITCODE -eq 0
            } 120
            Run docker (@('compose') + $guardCompose + @('run', '--rm', 'init', 'alembic', 'upgrade', 'head'))
            Run docker (@('compose') + $guardCompose + @('run', '--rm', 'init', 'python', '-m', 'app.modules.seed'))
            Run docker (@('compose') + $guardCompose + @('up', '-d'))
            Wait-ContainerHealthy 'guard preview readiness (8086)' {
                (Get-HttpStatus 'http://127.0.0.1:8086/health/ready') -eq 200
            } 180
            Run docker (@('compose') + $guardCompose + @('exec', '-T', 'postgres', 'psql',
                '-U', 'locker_rust', '-d', 'locker_rust', '-v', 'ON_ERROR_STOP=1', '-c', $markerSql))
            # Counts come back on stdout; an empty result means the query
            # never really ran and the invariant below would be vacuous.
            $script:GuardCounts = (docker compose @guardCompose exec -T postgres psql -U locker_rust -d locker_rust -tAc $script:GuardCountsSql) -join ''
            if ([string]::IsNullOrWhiteSpace($script:GuardCounts)) {
                throw 'guard counts query returned nothing; refusing a vacuous invariant'
            }
            Write-Output "guard marker and counts recorded: $($script:GuardCounts)"
        } finally {
            Remove-Item env:LOCKER_PREVIEW_PORT -ErrorAction SilentlyContinue
        }
    }

    Invoke-Step 'e2e-smoke' {
        # The complete cycle in its own project (locker-rust-preview-test,
        # port 8085): login, loan/return, presence, disconnect/reconnect,
        # broker and backend restarts, UNKNOWN/FAILED, reconciliation,
        # audit. Its finally clause cleans up only its own project.
        Run python @('scripts/test_rust_preview.py') $repo
    }

    Invoke-Step 'e2e-guard-verify' {
        try {
            $env:LOCKER_PREVIEW_PORT = '8086'
            $ready = Get-HttpStatus 'http://127.0.0.1:8086/health/ready'
            if ($ready -ne 200) { throw "guard preview is not ready anymore (status $ready)" }
            $after = (docker compose @guardCompose exec -T postgres psql -U locker_rust -d locker_rust -tAc $script:GuardCountsSql) -join ''
            if ([string]::IsNullOrWhiteSpace($after)) {
                throw 'guard counts query returned nothing; refusing a vacuous invariant'
            }
            if ($after -ne $script:GuardCounts) {
                throw "guard data changed during the smoke: $($script:GuardCounts) -> $after"
            }
            Write-Output "guard data preserved across the smoke: $after (marker included)"
        } finally {
            Run docker (@('compose') + $guardCompose + @('down', '-v'))
            Remove-Item env:LOCKER_PREVIEW_PORT -ErrorAction SilentlyContinue
        }
    }

    Invoke-Step 'e2e-laboratory-untouched' {
        $labNow = Get-HttpStatus 'http://127.0.0.1:8083/health/ready'
        $baseline = "$script:LabBaseline"
        if ($baseline -eq 'unreachable' -and $labNow -eq 'unreachable') {
            Write-Output 'laboratory 8083 was not running and is still not running: untouched'
        } elseif ($labNow -ne $baseline) {
            throw "laboratory health changed during E2E: $baseline -> $labNow"
        } else {
            Write-Output "laboratory 8083 health unchanged ($labNow); its data was never touched"
        }
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
if ($skipped.Count -gt 0) {
    Write-Host 'skipped steps and reasons (never silent):' -ForegroundColor DarkYellow
    $skipped | ForEach-Object { Write-Host "  - $($_.Step): $($_.Note)" }
}
Write-Host "evidence: $evidence"
if ($failed.Count -gt 0) {
    Write-Host 'check-local: FAILED' -ForegroundColor Red
    exit 1
}
Write-Host 'check-local: OK' -ForegroundColor Green
exit 0
