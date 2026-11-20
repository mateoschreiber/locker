#!/usr/bin/env python3
"""End-to-end smoke of the React panel against the Rust backend (F4.1-C/D).

Disposable stack via compose.rust-preview.yaml under its own project
(locker-rust-preview-test, port 8085 by default): isolated PostgreSQL with
the LAB schema and seed initialized explicitly, an isolated broker and
simulator, the Rust API and an nginx web container pointed at Rust. Nothing
from the laboratory stack is reused, and nothing from the interactive
preview (project locker-rust-preview, port 8084) is touched or destroyed —
if that preview happens to be running, its health and row counts are
snapshotted read-only before the scenario and asserted unchanged after it.

Scenarios: capabilities and login through nginx, observed presence (ONLINE
from the simulator, OFFLINE from its Last Will), a full checkout/return flow
confirmed by the simulator, a forced UNKNOWN by TTL (simulator stopped)
reconciled as FAILED with the loan released, and a real api-rust restart
mid-operation reconciled as CONFIRMED with the domain effects applied.

On any failure the script captures evidence (compose ps, logs of every
service, health/ready failure mode) under .local-validation/ BEFORE the
disposable teardown, so a CI failure can be diagnosed post-mortem.
"""

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

PROJECT = 'locker-rust-preview-test'
INTERACTIVE_PROJECT = 'locker-rust-preview'
COMPOSE = ['docker', 'compose', '-p', PROJECT, '-f', 'compose.rust-preview.yaml']
PORT = int(os.environ.get('LOCKER_PREVIEW_TEST_PORT', '8085'))
BASE = f'http://127.0.0.1:{PORT}'
EVIDENCE_ROOT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), '.local-validation')


def compose(*args, check=True):
    # LOCKER_PREVIEW_PORT drives the compose file's published port; the OS
    # environment takes precedence over any .env file, so the test project
    # can never collide with the interactive preview on 8084.
    env = {**os.environ, 'LOCKER_PREVIEW_PORT': str(PORT)}
    return subprocess.run([*COMPOSE, *args], check=check, capture_output=True, text=True, env=env)


def call(method, path, body=None, token=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(BASE + path, data=data, method=method)
    if data is not None:
        request.add_header('Content-Type', 'application/json')
    if token:
        request.add_header('Authorization', f'Bearer {token}')
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read() or b'null')
    except urllib.error.HTTPError as error:
        # During service restarts nginx answers with plain-HTML 502 pages;
        # the status code is what matters to the caller.
        try:
            body = json.loads(error.read() or b'{}')
        except (ValueError, UnicodeDecodeError):
            body = {}
        return error.code, body
    except (OSError, urllib.error.URLError):
        return None, {}


def probe(path):
    """One health observation with the failure mode spelled out: 200 ok,
    503 = api answered but not ready (DB/MQTT), 502 = nginx without its
    upstream (api restarting), refused/timeout = web container itself
    unreachable. Read-only diagnostics; never a password or token."""
    request = urllib.request.Request(BASE + path)
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            return f'{response.status} ok'
    except urllib.error.HTTPError as error:
        return {503: '503 api answered: not ready', 502: '502 nginx: upstream unavailable'}.get(
            error.code, f'{error.code} http error')
    except urllib.error.URLError as error:
        return f'no connection ({error.reason})'
    except OSError as error:
        return f'no connection ({error})'


def capture_evidence(stage):
    directory = os.path.join(
        EVIDENCE_ROOT, f'preview-smoke-{time.strftime("%Y%m%d-%H%M%S")}')
    os.makedirs(directory, exist_ok=True)
    def save(name, text):
        with open(os.path.join(directory, name), 'w', encoding='utf-8') as handle:
            handle.write(text or '')
    save('stage.txt', stage)
    state = compose('ps', check=False)
    save('compose-ps.txt', state.stdout + state.stderr)
    for service in ('api-rust', 'postgres', 'mosquitto', 'web-rust'):
        logs = compose('logs', '--no-color', '--tail', '300', service, check=False)
        save(f'logs-{service}.txt', logs.stdout + logs.stderr)
    save(
        'health.txt',
        f'stage: {stage}\nhealth/live: {probe("/health/live")}\n'
        f'health/ready: {probe("/health/ready")}\n')
    print(f'preview evidence captured in {directory}', file=sys.stderr)


def wait_until(description, predicate, seconds=90, observer=None):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            print(f'preview OK: {description}')
            return
        time.sleep(1)
    detail = f' [{observer()}]' if observer else ''
    raise RuntimeError(f'timeout waiting for {description}{detail}')


def ready_observer():
    return f'live: {probe("/health/live")}; ready: {probe("/health/ready")}'


def interactive_preview_running():
    result = subprocess.run(
        ['docker', 'compose', '-p', INTERACTIVE_PROJECT, '-f', 'compose.rust-preview.yaml',
         'ps', '--status', 'running'],
        capture_output=True, text=True)
    return bool(result.stdout.strip())


def interactive_preview_snapshot():
    """Read-only marker of the user's interactive preview: its health and
    its row counts. Nothing is inserted, modified or removed there."""
    counts = subprocess.run(
        ['docker', 'compose', '-p', INTERACTIVE_PROJECT, '-f', 'compose.rust-preview.yaml',
         'exec', '-T', 'postgres', 'psql', '-U', 'locker_rust', '-d', 'locker_rust', '-tAc',
         "SELECT (SELECT count(*) FROM branches) || '/' || (SELECT count(*) FROM tools) || "
         "'/' || (SELECT count(*) FROM users) || '/' || (SELECT count(*) FROM loans) || "
         "'/' || (SELECT count(*) FROM locker_operations)"],
        capture_output=True, text=True)
    return counts.stdout.strip()


def wait_status(operation_id, wanted, seconds=60):
    def reached():
        _, operation = call('GET', '/api/v1/operations')
        row = next((item for item in operation if item['id'] == operation_id), None)
        return row is not None and row['status'] == wanted
    wait_until(f'operation {operation_id} reaches {wanted}', reached, seconds)


def loan_status(loan_id):
    _, loans = call('GET', '/api/v1/loans')
    return next((item['status'] for item in loans if item['id'] == loan_id), None)


def main():
    watched = interactive_preview_running()
    snapshot = interactive_preview_snapshot() if watched else None
    if watched:
        print(f'interactive preview detected (project {INTERACTIVE_PROJECT}); '
              'its data must remain untouched by this smoke')
    else:
        print(f'interactive preview not running; read-only isolation check skipped')
    try:
        print('building the preview images...')
        compose('build')
        compose('up', '-d', 'postgres', 'mosquitto')
        wait_until('isolated PostgreSQL', lambda: compose(
            'exec', '-T', 'postgres', 'pg_isready', '-U', 'locker_rust', '-d', 'locker_rust',
            check=False).returncode == 0)
        wait_until('isolated MQTT', lambda: compose(
            'exec', '-T', 'mosquitto', 'mosquitto_sub', '-h', 'localhost',
            '-t', '$SYS/broker/version', '-C', '1', '-W', '3', check=False).returncode == 0)
        # Explicit schema + LAB seed through the Python image (lab pattern;
        # the seed refuses a non-empty database).
        compose('run', '--rm', 'init', 'alembic', 'upgrade', 'head')
        compose('run', '--rm', 'init', 'python', '-m', 'app.modules.seed')
        compose('up', '-d', 'simulator', 'api-rust', 'web-rust')

        wait_until('web /health/ready 200 (Rust subscribed)', lambda: call('GET', '/health/ready')[0] == 200,
                   observer=ready_observer)

        # 1. Capabilities: the panel feature gate.
        status, caps = call('GET', '/api/v1/capabilities')
        assert status == 200 and caps.get('resolve') and caps.get('presence'), caps
        print('preview OK: capabilities advertise resolve and presence')

        # 2. Lab login through nginx; the token drives reconciliation.
        status, session = call('POST', '/api/v1/auth/login',
                               {'username': 'admin', 'password': 'admin'})
        assert status == 200 and session.get('access_token'), session
        token = session['access_token']
        print('preview OK: login through nginx')

        # 3. Observed presence: the simulator announces itself (retained
        # LOCKER_ONLINE, fresh) — never an invented availability.
        def locker_online():
            _, rows = call('GET', '/api/v1/lockers/presence')
            return any(row['code'] == 'LAB-LOCKER-001' and row['presence'] == 'ONLINE'
                       and row['age_seconds'] is not None and row['age_seconds'] < 90
                       for row in rows)
        wait_until('presence ONLINE for LAB-LOCKER-001', locker_online)

        # 4. Full loan flow confirmed by the simulator.
        _, tools = call('GET', '/api/v1/tools')
        _, users = call('GET', '/api/v1/users')
        _, branches = call('GET', '/api/v1/branches')
        tool, user, branch = tools[0], users[0], branches[0]
        status, authorization = call('POST', '/api/v1/authorizations', {
            'tool_id': tool['id'], 'user_id': user['id'], 'branch_id': branch['id']})
        assert status == 201, authorization
        assert call('POST', f"/api/v1/authorizations/{authorization['id']}/approve")[0] == 200
        status, checkout = call('POST', f"/api/v1/authorizations/{authorization['id']}/checkout")
        assert status == 202, checkout
        wait_status(checkout['id'], 'CONFIRMED')
        assert loan_status(checkout['loan_id']) == 'ACTIVE', 'checkout did not activate the loan'
        status, returned = call('POST', f"/api/v1/loans/{checkout['loan_id']}/return")
        assert status == 202, returned
        wait_status(returned['id'], 'CONFIRMED')
        assert loan_status(checkout['loan_id']) == 'RETURNED', 'return did not close the loan'
        print('preview OK: checkout and return confirmed by the simulator')

        # 5. Losing the simulator: the Last Will turns presence OFFLINE and
        # an ordered operation becomes UNKNOWN by TTL (5 s) — resolvable.
        compose('stop', 'simulator')
        def locker_offline():
            _, rows = call('GET', '/api/v1/lockers/presence')
            return any(row['code'] == 'LAB-LOCKER-001' and row['presence'] == 'OFFLINE'
                       for row in rows)
        wait_until('presence OFFLINE after simulator loss', locker_offline)

        # 5b. Broker loss and restart: the bridge reconnects (readiness again
        # gated on the events subscription) and the persisted retained OFFLINE
        # replays — an old message that can never fake freshness.
        compose('restart', 'mosquitto')
        wait_until('api-rust ready after broker restart', lambda: call('GET', '/health/ready')[0] == 200, seconds=180, observer=ready_observer)
        wait_until('presence OFFLINE restored by retained replay', locker_offline)

        status, authorization2 = call('POST', '/api/v1/authorizations', {
            'tool_id': tool['id'], 'user_id': user['id'], 'branch_id': branch['id']})
        call('POST', f"/api/v1/authorizations/{authorization2['id']}/approve")
        status, pending = call('POST', f"/api/v1/authorizations/{authorization2['id']}/checkout")
        assert status == 202, pending
        wait_status(pending['id'], 'UNKNOWN', seconds=60)
        status, resolved = call('POST', f"/api/v1/operations/{pending['id']}/resolve", {
            'outcome': 'FAILED', 'reason': 'simulador detenido',
            'evidence': 'escenario de smoke: sin confirmación física',
        }, token=token)
        assert status == 200 and resolved['status'] == 'FAILED', resolved
        assert loan_status(pending['loan_id']) == 'CANCELLED', 'resolve did not release the loan'
        status, reusable = call('POST', f"/api/v1/authorizations/{authorization2['id']}/checkout")
        assert status == 202, reusable
        wait_status(reusable['id'], 'UNKNOWN')
        status, applied = call('POST', f"/api/v1/operations/{reusable['id']}/resolve", {
            'outcome': 'CONFIRMED', 'reason': 'retiro verificado en sitio',
            'evidence': 'escenario de smoke: atestación del operador',
        }, token=token)
        assert status == 200 and applied['status'] == 'CONFIRMED', applied
        assert loan_status(reusable['loan_id']) == 'ACTIVE', 'confirmed attestation did not activate the loan'
        # Domain transitions verified against the real state, not just the
        # HTTP response: a correctly applied CHECKOUT keeps the authorization
        # CONSUMED (only a revert may return it to APPROVED).
        _, authorizations = call('GET', '/api/v1/authorizations')
        assert next((item['status'] for item in authorizations
                     if item['id'] == authorization2['id']), None) == 'CONSUMED', \
            'confirmed checkout must keep the authorization consumed'
        print('preview OK: UNKNOWN by TTL reconciled as CONFIRMED, loan active, authorization consumed')

        # 6. Real api-rust restart with an operation in flight (a return,
        # simulator still down): whatever the pre-restart state, recovery ends
        # in UNKNOWN, and a CONFIRMED attestation applies the domain effects
        # without any MQTT event.
        status, returning = call('POST', f"/api/v1/loans/{reusable['loan_id']}/return")
        assert status == 202, returning
        compose('restart', 'api-rust')
        wait_until('api-rust ready after mid-operation restart', lambda: call('GET', '/health/ready')[0] == 200, seconds=180, observer=ready_observer)
        wait_status(returning['id'], 'UNKNOWN', seconds=60)
        status, applied = call('POST', f"/api/v1/operations/{returning['id']}/resolve", {
            'outcome': 'CONFIRMED', 'reason': 'herramienta devuelta en sitio',
            'evidence': 'escenario de smoke: atestación del operador tras reinicio',
        }, token=token)
        assert status == 200 and applied['status'] == 'CONFIRMED', applied
        assert loan_status(reusable['loan_id']) == 'RETURNED', 'confirmed attestation did not close the loan'
        _, audit = call('GET', '/api/v1/audit')
        assert any(item['action'] == 'OPERATION_RESOLVED_CONFIRMED' for item in audit), \
            'resolution missing from the audit trail'
        print('preview OK: real restart mid-operation -> UNKNOWN -> CONFIRMED with domain effects and audit')

        if watched:
            after = interactive_preview_snapshot()
            assert after == snapshot, \
                f'interactive preview data changed during the smoke: {snapshot} -> {after}'
            print(f'preview OK: interactive preview data preserved ({snapshot})')

        print('Rust preview smoke: capabilities, presence, loan flow, TTL UNKNOWN, '
              'reconciliation and real restart: OK')
    except Exception as error:
        capture_evidence(f'{error.__class__.__name__}: {error}')
        raise
    finally:
        if not os.environ.get('LOCKER_PREVIEW_KEEP'):
            compose('down', '-v', check=False)


if __name__ == '__main__':
    try:
        main()
    except RuntimeError as error:
        print(f'preview FAILED: {error}', file=sys.stderr)
        sys.exit(1)
