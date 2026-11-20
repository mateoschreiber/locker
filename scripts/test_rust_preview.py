#!/usr/bin/env python3
"""End-to-end smoke of the React panel against the Rust backend (F4.1-C).

Disposable stack via compose.rust-preview.yaml: isolated PostgreSQL with the
LAB schema and seed initialized explicitly, an isolated broker and simulator,
the Rust API and an nginx web container pointed at Rust. Nothing from the
laboratory stack is reused (separate project, network, broker and volumes).

Scenarios: capabilities and login through nginx, observed presence (ONLINE
from the simulator, OFFLINE from its Last Will), a full checkout/return flow
confirmed by the simulator, a forced UNKNOWN by TTL (simulator stopped)
reconciled as FAILED with the loan released, and a real api-rust restart
mid-operation reconciled as CONFIRMED with the domain effects applied.
"""

import json
import subprocess
import sys
import time
import urllib.error
import urllib.request

PROJECT = 'locker-rust-preview'
COMPOSE = ['docker', 'compose', '-p', PROJECT, '-f', 'compose.rust-preview.yaml']
BASE = 'http://127.0.0.1:8084'
DB_URL = 'postgresql+psycopg://locker_rust:rust_test_only@postgres:5432/locker_rust'


def compose(*args, check=True):
    return subprocess.run([*COMPOSE, *args], check=check, capture_output=True, text=True)


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
        return error.code, json.loads(error.read() or b'{}')
    except (OSError, urllib.error.URLError):
        return None, {}


def wait_until(description, predicate, seconds=90):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            print(f'preview OK: {description}')
            return
        time.sleep(1)
    raise RuntimeError(f'timeout waiting for {description}')


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

        wait_until('web /health/ready 200 (Rust subscribed)', lambda: call('GET', '/health/ready')[0] == 200)

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
        print('preview OK: UNKNOWN by TTL reconciled as CONFIRMED, loan active, authorization reusable')

        # 6. Real api-rust restart with an operation in flight (a return,
        # simulator still down): whatever the pre-restart state, recovery ends
        # in UNKNOWN, and a CONFIRMED attestation applies the domain effects
        # without any MQTT event.
        status, returning = call('POST', f"/api/v1/loans/{reusable['loan_id']}/return")
        assert status == 202, returning
        compose('restart', 'api-rust')
        wait_until('api-rust ready after mid-operation restart', lambda: call('GET', '/health/ready')[0] == 200)
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

        print('Rust preview smoke: capabilities, presence, loan flow, TTL UNKNOWN, '
              'reconciliation and real restart: OK')
    finally:
        compose('down', '-v', check=False)


if __name__ == '__main__':
    try:
        main()
    except RuntimeError as error:
        print(f'preview FAILED: {error}', file=sys.stderr)
        sys.exit(1)
