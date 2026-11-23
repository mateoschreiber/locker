#!/usr/bin/env python3
"""Phase 2 parity harness: Python and Rust APIs against the same isolated
PostgreSQL + MQTT stack, comparing responses after driving the flow via Rust.

Coverage:
- read parity: all entity lists, dashboard, locker detail, tool history;
- write parity: login, authorization, approve, checkout, return (via Rust)
  with the resulting state read back through both APIs;
- error parity: same status + same `detail` message on the shared contract;
- deliberate deviation (required by the Phase 2 idempotency mandate): a
  retried checkout/return returns the existing operation (202) instead of
  Python's 409; asserted as such, not as a failure.
"""

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from uuid import uuid4

API_IMAGE = 'locker-parity-api'
RUST_IMAGE = 'locker-parity-api-rust'
PREFIX = f'locker-parity-{uuid4().hex[:12]}'
NETWORK = PREFIX
POSTGRES = f'{PREFIX}-pg'
MOSQUITTO = f'{PREFIX}-mqtt'
PYTHON_API = f'{PREFIX}-python'
RUST_API = f'{PREFIX}-rust'
DB_URL = 'postgresql+psycopg://locker_parity:parity_only@pg:5432/locker_parity'
RUST_DB_URL = 'postgres://locker_parity:parity_only@pg:5432/locker_parity'
created = []


def docker(*args, check=True):
    result = subprocess.run(
        ['docker', *args], capture_output=True, text=True, encoding='utf-8',
        errors='replace', check=False, timeout=600,
    )
    if check and result.returncode:
        raise RuntimeError(f'docker {args[:2]} failed: {result.stderr.strip()}')
    return result


def wait_until(description, predicate, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.5)
    for container in created:
        logs = docker('logs', '--tail', '5', container, check=False).stdout
        print(f'--- {container} ---\n{logs}')
    raise RuntimeError(f'timeout waiting for {description}')


def http(method, url, body=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(url, data=data, method=method)
    if data:
        request.add_header('Content-Type', 'application/json')
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())
    except OSError:
        # Startup phase: connection accepted then closed, or not yet listening.
        return None, None


def normalize_item(item):
    """Drop volatile keys: timestamps differ between serializers and the
    Python users dump still includes the password (lab P0, Phase 5)."""
    return {
        key: value
        for key, value in item.items()
        if not key.endswith('_at') and key != 'password'
    }


def normalize(value):
    if isinstance(value, list):
        return sorted((normalize_item(v) for v in value), key=lambda v: str(v.get('id', '')))
    if isinstance(value, dict):
        # Same rules as normalize_item: volatile timestamps and the lab password
        # (Python still serializes it; Rust deliberately does not).
        return {
            key: normalize(item)
            for key, item in value.items()
            if not key.endswith('_at') and key != 'password'
        }
    return value


def compare(label, python_value, rust_value):
    if normalize(python_value) != normalize(rust_value):
        print(f'PARITY MISMATCH {label}')
        print('python:', json.dumps(normalize(python_value), indent=1, sort_keys=True)[:2000])
        print('rust  :', json.dumps(normalize(rust_value), indent=1, sort_keys=True)[:2000])
        raise SystemExit(1)
    print(f'parity OK: {label}')


def read_parity(python_base, rust_base, label):
    for path in [
        '/api/v1/branches', '/api/v1/roles', '/api/v1/users', '/api/v1/memberships',
        '/api/v1/lockers', '/api/v1/compartments', '/api/v1/locks', '/api/v1/cameras',
        '/api/v1/tools', '/api/v1/placements', '/api/v1/authorizations', '/api/v1/loans',
        '/api/v1/operations', '/api/v1/audit', '/api/v1/dashboard',
    ]:
        python_status, python_value = http('GET', python_base + path)
        rust_status, rust_value = http('GET', rust_base + path)
        assert python_status == rust_status == 200, f'{path}: {python_status} vs {rust_status}'
        compare(f'{label} {path}', python_value, rust_value)


def error_parity(python_base, rust_base, label, method, path, body=None, expected=409):
    python_status, python_value = http(method, python_base + path, body)
    rust_status, rust_value = http(method, rust_base + path, body)
    assert python_status == rust_status == expected, (
        f'{label}: python {python_status} ({python_value}) vs rust {rust_status} ({rust_value})'
    )
    assert python_value['detail'] == rust_value['detail'], (
        f"{label}: detail mismatch python={python_value['detail']!r} rust={rust_value['detail']!r}"
    )
    print(f'parity OK: {label} -> {expected} {python_value["detail"]}')


def main():
    try:
        docker('build', 'apps/api', '-t', API_IMAGE)
        docker('build', 'apps/api-rust', '-t', RUST_IMAGE)
        docker('network', 'create', NETWORK)

        docker('run', '-d', '--name', POSTGRES, '--network', NETWORK, '--network-alias', 'pg',
               '-e', 'POSTGRES_USER=locker_parity', '-e', 'POSTGRES_PASSWORD=parity_only',
               '-e', 'POSTGRES_DB=locker_parity', 'postgres:17-alpine')
        created.append(POSTGRES)
        wait_until('isolated PostgreSQL', lambda: docker(
            'exec', POSTGRES, 'pg_isready', '-U', 'locker_parity', '-d', 'locker_parity',
            check=False).returncode == 0)
        docker('run', '-d', '--name', MOSQUITTO, '--network', NETWORK,
               '--network-alias', 'mosquitto',
               '-v', f'{os.getcwd()}/infra/docker/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro',
               'eclipse-mosquitto:2.0.22')
        created.append(MOSQUITTO)
        wait_until('isolated MQTT', lambda: docker(
            'exec', MOSQUITTO, 'mosquitto_sub', '-h', 'localhost', '-t', '$SYS/broker/version',
            '-C', '1', '-W', '3', check=False).returncode == 0, seconds=90)

        docker('run', '--rm', '--network', NETWORK, '-e', f'DATABASE_URL={DB_URL}',
               API_IMAGE, 'alembic', 'upgrade', 'head')
        docker('run', '--rm', '--network', NETWORK, '-e', f'DATABASE_URL={DB_URL}',
               '-e', 'APP_ENV=lab', API_IMAGE, 'python', '-m', 'app.modules.seed')

        docker('run', '-d', '--name', PYTHON_API, '--network', NETWORK,
               '-p', '127.0.0.1::8000', '-e', f'DATABASE_URL={DB_URL}',
               '-e', 'MQTT_HOST=mosquitto', '-e', 'APP_ENV=lab', API_IMAGE)
        created.append(PYTHON_API)
        docker('run', '-d', '--name', RUST_API, '--network', NETWORK,
               '-p', '127.0.0.1::8001', '-e', f'RUST_DATABASE_URL={RUST_DB_URL}',
               '-e', 'RUST_BIND_ADDRESS=0.0.0.0:8001',
               # Paridad REST: el puente MQTT de Rust queda apagado para que las
               # operaciones queden PENDING como en Fase 2; el ciclo MQTT se
               # prueba con broker real en scripts/test_rust_mqtt.py.
               '-e', 'RUST_MQTT_ENABLED=false', RUST_IMAGE)
        created.append(RUST_API)

        def host_port(container, port):
            return docker('inspect', '-f',
                          f'{{{{(index (index .NetworkSettings.Ports "{port}/tcp") 0).HostPort}}}}',
                          container).stdout.strip()

        python_base = f'http://127.0.0.1:{host_port(PYTHON_API, 8000)}'
        rust_base = f'http://127.0.0.1:{host_port(RUST_API, 8001)}'
        wait_until('python /health/ready', lambda: http('GET', python_base + '/health/ready')[0] == 200)
        wait_until('rust /health/ready', lambda: http('GET', rust_base + '/health/ready')[0] == 200)

        # 1. Read parity over the seeded laboratory.
        read_parity(python_base, rust_base, 'seeded')

        # 2. Login parity (same token, same detail on failure, no password in Rust users).
        status, python_login = http('POST', python_base + '/api/v1/auth/login',
                                    {'username': 'admin', 'password': 'admin'})
        status, rust_login = http('POST', rust_base + '/api/v1/auth/login',
                                  {'username': 'admin', 'password': 'admin'})
        assert python_login['access_token'] == rust_login['access_token'] == 'lab-admin-token'
        compare('login user', python_login['user'], rust_login['user'])
        error_parity(python_base, rust_base, 'login wrong password', 'POST', '/api/v1/auth/login',
                     {'username': 'admin', 'password': 'nope'}, expected=401)

        # Shared entities for the write flow.
        _, branches = http('GET', rust_base + '/api/v1/branches')
        _, users = http('GET', rust_base + '/api/v1/users')
        _, tools = http('GET', rust_base + '/api/v1/tools')
        branch, user, tool = branches[0], users[0], tools[0]
        _, lockers = http('GET', rust_base + '/api/v1/lockers')
        locker = lockers[0]

        # 3. Error parity on the shared contract.
        error_parity(python_base, rust_base, 'get unknown branch', 'GET',
                     f'/api/v1/branches/{uuid4()}', expected=404)
        error_parity(python_base, rust_base, 'deactivate unknown role', 'POST',
                     f'/api/v1/roles/{uuid4()}/deactivate', expected=404)
        _, roles = http('GET', rust_base + '/api/v1/roles')
        error_parity(python_base, rust_base, 'deactivate role', 'POST',
                     f"/api/v1/roles/{roles[0]['id']}/deactivate")
        error_parity(python_base, rust_base, 'duplicate placement', 'POST', '/api/v1/placements',
                     {'tool_id': tool['id'], 'branch_id': branch['id'],
                      'locker_id': locker['id'], 'compartment_id':
                      http('GET', rust_base + '/api/v1/placements')[1][0]['compartment_id']})

        # 4. Write flow driven through the Rust API.
        _, authorization = http('POST', rust_base + '/api/v1/authorizations',
                                {'tool_id': tool['id'], 'user_id': user['id'],
                                 'branch_id': branch['id']})
        assert authorization['status'] == 'PENDING', authorization
        error_parity(python_base, rust_base, 'checkout pending authorization', 'POST',
                     f"/api/v1/authorizations/{authorization['id']}/checkout")
        status, approved = http('POST', rust_base + f"/api/v1/authorizations/{authorization['id']}/approve")
        assert status == 200 and approved['status'] == 'APPROVED', approved
        error_parity(python_base, rust_base, 'approve twice', 'POST',
                     f"/api/v1/authorizations/{authorization['id']}/approve")
        status, operation = http('POST', rust_base + f"/api/v1/authorizations/{authorization['id']}/checkout")
        assert status == 202 and operation['status'] == 'PENDING', operation
        status, retry = http('POST', rust_base + f"/api/v1/authorizations/{authorization['id']}/checkout")
        # Required deviation: Rust answers idempotently with the same operation.
        assert status == 202 and retry['id'] == operation['id'], retry
        status, python_retry = http('POST', python_base + f"/api/v1/authorizations/{authorization['id']}/checkout")
        assert status == 409, python_retry
        read_parity(python_base, rust_base, 'after checkout')

        # 5. Simulate the MQTT confirmation (Phase 3 migrates the bridge).
        confirm = """
            UPDATE tool_placements SET removed_at = now() WHERE tool_id = '%s' AND removed_at IS NULL;
            UPDATE loans SET status = 'ACTIVE', checked_out_at = now() WHERE id = '%s';
            UPDATE tools SET status = 'ON_LOAN' WHERE id = '%s';
        """ % (tool['id'], operation['loan_id'], tool['id'])
        docker('exec', POSTGRES, 'psql', '-U', 'locker_parity', '-d', 'locker_parity',
               '-c', confirm)
        status, returned = http('POST', rust_base + f"/api/v1/loans/{operation['loan_id']}/return")
        assert status == 202 and returned['status'] == 'PENDING', returned
        status, return_retry = http('POST', rust_base + f"/api/v1/loans/{operation['loan_id']}/return")
        assert status == 202 and return_retry['id'] == returned['id'], return_retry
        error_parity(python_base, rust_base, 'return non-active loan', 'POST',
                     f"/api/v1/loans/{uuid4()}/return")
        read_parity(python_base, rust_base, 'after return')

        # 6. F4.0 status contract (#19): identical 422/409 on both backends.
        error_parity(python_base, rust_base, 'unknown status', 'PATCH',
                     f"/api/v1/branches/{branch['id']}", {'status': 'BANANA'}, expected=422)
        error_parity(python_base, rust_base, 'locker administrative states', 'PATCH',
                     f"/api/v1/lockers/{locker['id']}", {'status': 'ACTIVE'}, expected=422)
        error_parity(python_base, rust_base, 'domain-only tool status', 'POST',
                     '/api/v1/tools',
                     {'asset_code': f'TOOL-{uuid4().hex[:8]}', 'name': 'Paridad',
                      'status': 'ON_LOAN'}, expected=422)
        # The tool has an in-flight loan: manual writes are refused in both.
        error_parity(python_base, rust_base, 'tool with in-flight loan', 'PATCH',
                     f"/api/v1/tools/{tool['id']}", {'status': 'INACTIVE'})
        error_parity(python_base, rust_base, 'deactivate tool with in-flight loan', 'POST',
                     f"/api/v1/tools/{tool['id']}/deactivate")
        # A compartment holding a placement refuses manual deactivation.
        _, compartments = http('GET', rust_base + '/api/v1/compartments')
        error_parity(python_base, rust_base, 'compartment with placement', 'POST',
                     f"/api/v1/compartments/{compartments[0]['id']}/deactivate")

        # F4.0-fix: status edge parity. Rust treats null as an absent field and
        # rejects non-strings as a type error; Python must answer identically.
        for base in (python_base, rust_base):
            status, value = http('PATCH', base + f"/api/v1/branches/{branch['id']}",
                                 {'status': None})
            assert status == 200, f'null status PATCH: {status} ({value})'
            assert value['status'] == branch['status'], value
            code = f'NULA-{uuid4().hex[:8]}'
            status, value = http('POST', base + '/api/v1/branches',
                                 {'code': code, 'name': 'Status nulo', 'status': None})
            assert status == 201, f'null status create: {status} ({value})'
            assert value['status'] == 'ACTIVE', value
        for invalid in (5, True, ['ACTIVE'], {'code': 'X'}):
            error_parity(python_base, rust_base, f'non-string status {invalid!r}', 'PATCH',
                         f"/api/v1/branches/{branch['id']}", {'status': invalid},
                         expected=422)
        error_parity(python_base, rust_base, 'empty status', 'PATCH',
                     f"/api/v1/branches/{branch['id']}", {'status': ''}, expected=422)

        # Inactive entities reject authorization in both backends with the same 409 detail.
        _, inactive_tool = http('POST', rust_base + '/api/v1/tools',
                                {'asset_code': f'INACT-{uuid4().hex[:8]}',
                                 'name': 'Inactiva', 'status': 'INACTIVE'})
        error_parity(python_base, rust_base, 'authorize inactive tool', 'POST',
                     '/api/v1/authorizations',
                     {'tool_id': inactive_tool['id'], 'user_id': user['id'],
                      'branch_id': branch['id']}, expected=409)

        # 7. Parity of the detail endpoints after the flow.
        _, python_detail = http('GET', python_base + f"/api/v1/lockers/{locker['id']}/detail")
        _, rust_detail = http('GET', rust_base + f"/api/v1/lockers/{locker['id']}/detail")
        compare('locker detail', python_detail, rust_detail)
        _, python_history = http('GET', python_base + f"/api/v1/tools/{tool['id']}/history")
        _, rust_history = http('GET', rust_base + f"/api/v1/tools/{tool['id']}/history")
        compare('tool history', python_history, rust_history)

        print('Phase 2 parity: read, write and error contracts equivalent. OK')
    finally:
        for name in reversed(created):
            docker('rm', '-f', name, check=False)
        docker('network', 'rm', NETWORK, check=False)


if __name__ == '__main__':
    main()
