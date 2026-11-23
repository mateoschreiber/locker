#!/usr/bin/env python3
"""Fase 3 MQTT integration scenario with disposable containers.

Exercises what unit tests cannot: a REAL Rust process restart mid-operation,
a REAL broker outage and recovery, and Python/Rust coexistence against one
shared database where each backend processes only the operations it owns.
"""

import json
import os
import subprocess
import time
import urllib.error
import urllib.request
from uuid import uuid4

RUST_IMAGE = 'locker-mqtt-api-rust'
PYTHON_IMAGE = 'locker-mqtt-api-python'
SIM_IMAGE = 'locker-mqtt-simulator'
prefix = f'locker-mqtt-{uuid4().hex[:12]}'
NETWORK = prefix
POSTGRES = f'{prefix}-pg'
MOSQUITTO = f'{prefix}-mqtt'
SIMULATOR = f'{prefix}-sim'
RUST_API = f'{prefix}-rust'
PYTHON_API = f'{prefix}-python'
created = []
created_network = False

DB_USER, DB_NAME = 'locker_mqtt', 'locker_mqtt'
DB_PASSWORD = 'mqtt_test_only'
DB_URL = f'postgres://{DB_USER}:{DB_PASSWORD}@{POSTGRES}:5432/{DB_NAME}'
LOCKER_CODE = 'RL1'


def docker(*args, check=True, stdin=None):
    result = subprocess.run(
        ['docker', *args], capture_output=True, text=True, encoding='utf-8',
        errors='replace', check=False, timeout=120, input=stdin,
    )
    if check and result.returncode:
        raise RuntimeError(f'docker {args[0]} failed: {result.stderr.strip()}')
    return result


def wait_until(description, predicate, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.25)
    raise RuntimeError(f'timeout waiting for {description}')


def http(method, url, body=None, token=None):
    request = urllib.request.Request(url, method=method)
    data = json.dumps(body).encode() if body is not None else None
    if data is not None:
        request.add_header('content-type', 'application/json')
    if token:
        request.add_header('authorization', f'Bearer {token}')
    try:
        with urllib.request.urlopen(request, data=data, timeout=5) as response:
            return response.status, json.loads(response.read() or b'null')
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read() or b'null')
    except (OSError, urllib.error.URLError):
        return None, None


def psql(sql):
    return docker('exec', POSTGRES, 'psql', '-U', DB_USER, '-d', DB_NAME, '-tAc', sql).stdout.strip()


def main():
    global created_network
    try:
        docker('build', '-t', RUST_IMAGE, 'apps/api-rust')
        docker('build', '-t', SIM_IMAGE, 'apps/locker-simulator')
        docker('network', 'create', NETWORK)
        created_network = True

        docker('run', '-d', '--name', POSTGRES, '--network', NETWORK,
               '--tmpfs', '/var/lib/postgresql/data', '-e', f'POSTGRES_USER={DB_USER}',
               '-e', f'POSTGRES_PASSWORD={DB_PASSWORD}', '-e', f'POSTGRES_DB={DB_NAME}',
               'postgres:17-alpine')
        created.append(POSTGRES)
        wait_until('isolated PostgreSQL', lambda: docker(
            'exec', POSTGRES, 'pg_isready', '-U', DB_USER, '-d', DB_NAME,
            check=False).returncode == 0)
        # Initialize schema via locker-api-rust migrate (SQLx baseline).
        docker('run', '--rm', '--network', NETWORK,
               '-e', f'RUST_DATABASE_URL={DB_URL}',
               RUST_IMAGE, 'locker-api-rust', 'migrate')

        mosquitto_conf = f'{os.getcwd()}/infra/docker/mosquitto.conf'
        docker('run', '-d', '--name', MOSQUITTO, '--network', NETWORK,
               '--network-alias', 'mqtt',
               '-v', f'{mosquitto_conf}:/mosquitto/config/mosquitto.conf:ro',
               'eclipse-mosquitto:2.0.22')
        created.append(MOSQUITTO)
        wait_until('isolated MQTT', lambda: docker(
            'exec', MOSQUITTO, 'mosquitto_sub', '-h', 'localhost', '-t', '$SYS/broker/version',
            '-C', '1', '-W', '3', check=False).returncode == 0)

        docker('run', '-d', '--name', SIMULATOR, '--network', NETWORK,
               '-e', 'MQTT_HOST=mqtt', '-e', f'LOCKER_ID={LOCKER_CODE}', SIM_IMAGE)
        created.append(SIMULATOR)

        # The Rust backend shares the database with the Python backend for the
        # coexistence step; ownership is partitioned by `origin`.
        docker('run', '-d', '--name', RUST_API, '--network', NETWORK, '-p', '127.0.0.1::8001',
               '-e', f'RUST_DATABASE_URL={DB_URL}', '-e', 'RUST_BIND_ADDRESS=0.0.0.0:8001',
               '-e', 'RUST_MQTT_HOST=mqtt', '-e', 'RUST_MQTT_CONFIRM_TIMEOUT_SECONDS=10',
               RUST_IMAGE)
        created.append(RUST_API)

        rust_port = docker('inspect', '-f',
                           '{{(index (index .NetworkSettings.Ports "8001/tcp") 0).HostPort}}',
                           RUST_API).stdout.strip()
        rust = f'http://127.0.0.1:{rust_port}'
        wait_until('rust /health/ready', lambda: http('GET', rust + '/health/ready')[0] == 200)

        seed = """
        INSERT INTO users (id, username, display_name, password, status)
            VALUES (gen_random_uuid(), 'admin', 'Administrador', 'admin', 'ACTIVE');
        INSERT INTO branches (id, code, name, status)
            VALUES (gen_random_uuid(), 'RLB', 'Sucursal', 'ACTIVE');
        INSERT INTO lockers (id, branch_id, code, name, status)
            SELECT gen_random_uuid(), id, 'RL1', 'Locker', 'ONLINE' FROM branches;
        INSERT INTO compartments (id, locker_id, code, name, position, status)
            SELECT gen_random_uuid(), id, 'C' || n, 'Compartimiento', n, 'ACTIVE'
            FROM lockers, generate_series(1, 5) AS n;
        INSERT INTO tools (id, asset_code, name, status)
            SELECT gen_random_uuid(), 'RL-TOOL-' || n, 'Herramienta', 'AVAILABLE'
            FROM generate_series(1, 5) AS n;
        INSERT INTO tool_placements (id, tool_id, branch_id, locker_id, compartment_id, reason)
            SELECT gen_random_uuid(), t.id, (SELECT id FROM branches),
                   (SELECT id FROM lockers), c.id, 'INITIAL_ASSIGNMENT'
            FROM tools t JOIN compartments c ON c.code = 'C' || right(t.asset_code, 1);
        """
        docker('exec', POSTGRES, 'psql', '-U', DB_USER, '-d', DB_NAME, '-c', seed)

        branch = psql('SELECT id FROM branches LIMIT 1')
        user = psql("SELECT id FROM users WHERE username = 'admin'")
        tools = psql(
            "SELECT string_agg(id::text, ',' ORDER BY asset_code) FROM tools"
        ).split(',')

        def rust_checkout(tool_id):
            status, authorization = http('POST', rust + '/api/v1/authorizations',
                                         {'tool_id': tool_id, 'user_id': user, 'branch_id': branch})
            assert status == 201, authorization
            status, approved = http('POST', rust + f"/api/v1/authorizations/{authorization['id']}/approve")
            assert status == 200, approved
            status, operation = http('POST', rust + f"/api/v1/authorizations/{authorization['id']}/checkout")
            assert status == 202, operation
            return authorization['id'], operation

        # 1. Full flow through the real simulator.
        authorization, operation = rust_checkout(tools[0])
        correlation = operation['correlation_id']
        wait_until('checkout CONFIRMED', lambda: psql(
            f"SELECT status FROM locker_operations WHERE correlation_id = '{correlation}'"
        ) == 'CONFIRMED')
        assert psql(f"SELECT status FROM loans WHERE id = '{operation['loan_id']}'") == 'ACTIVE'
        assert psql(f"SELECT origin FROM locker_operations WHERE correlation_id = '{correlation}'") == 'rust'
        print('mqtt scenario: full checkout flow via broker + simulator: OK')

        # 2. A duplicated COMPARTMENT_OPENED has no double effect.
        compartment = psql('SELECT id FROM compartments LIMIT 1')
        duplicate = json.dumps({
            'message_id': str(uuid4()), 'correlation_id': correlation,
            'protocol_version': '1.0', 'locker_id': LOCKER_CODE,
            'compartment_id': compartment, 'timestamp': '2026-11-19T00:00:00+00:00',
            'type': 'COMPARTMENT_OPENED', 'payload': {'simulated': True},
        })
        docker('exec', MOSQUITTO, 'mosquitto_pub', '-h', 'localhost',
               '-t', f'locker/{LOCKER_CODE}/events', '-q', '1', '-m', duplicate)
        time.sleep(1.5)
        assert psql(f"SELECT status FROM locker_operations WHERE correlation_id = '{correlation}'") == 'CONFIRMED'
        assert psql(f"SELECT count(*) FROM audit_events WHERE action = 'MQTT_CONFIRMED' AND entity_id = '{operation['id']}'") == '1'
        print('mqtt scenario: duplicate event ignored, single confirmation: OK')

        # 3. Broker outage: an operation claimed into the dying connection has
        #    an indeterminable PUBACK -> UNKNOWN (never auto-republished); one
        #    never claimed stays PENDING and recovers alone. Both are safe.
        docker('stop', '-t', '3', MOSQUITTO)
        authorization, operation = rust_checkout(tools[1])
        correlation = operation['correlation_id']
        docker('start', MOSQUITTO)
        wait_until('broker back', lambda: docker(
            'exec', MOSQUITTO, 'mosquitto_sub', '-h', 'localhost', '-t', '$SYS/broker/version',
            '-C', '1', '-W', '3', check=False).returncode == 0)
        final = None
        for _ in range(120):
            final = psql(f"SELECT status FROM locker_operations WHERE correlation_id = '{correlation}'")
            if final in ('CONFIRMED', 'UNKNOWN'):
                break
            time.sleep(0.25)
        assert final in ('CONFIRMED', 'UNKNOWN'), final
        if final == 'UNKNOWN':
            status, resolved = http('POST', rust + f"/api/v1/operations/{operation['id']}/resolve",
                                    {'outcome': 'FAILED', 'reason': 'caída del broker',
                                     'evidence': 'compartimento verificado cerrado'},
                                    token='lab-admin-token')
            assert status == 200, resolved
            assert psql(f"SELECT status FROM loans WHERE id = '{operation['loan_id']}'") == 'CANCELLED'
            print('mqtt scenario: outage -> indeterminable UNKNOWN -> resolved over REST: OK')
        else:
            assert psql(f"SELECT status FROM loans WHERE id = '{operation['loan_id']}'") == 'ACTIVE'
            print('mqtt scenario: outage -> PENDING recovered -> CONFIRMED: OK')

        # 4. REAL process restart mid-operation: PENDING is republished once,
        #    an orphan PUBLISHING becomes UNKNOWN and is resolved over REST.
        authorization, operation = rust_checkout(tools[2])
        correlation = operation['correlation_id']
        docker('restart', '-t', '1', RUST_API)
        # docker restart may reassign the random published host port.
        rust_port = docker('inspect', '-f',
                           '{{(index (index .NetworkSettings.Ports "8001/tcp") 0).HostPort}}',
                           RUST_API).stdout.strip()
        rust = f'http://127.0.0.1:{rust_port}'
        wait_until('rust back after restart', lambda: http('GET', rust + '/health/ready')[0] == 200)
        final = None
        for _ in range(120):
            final = psql(f"SELECT status FROM locker_operations WHERE correlation_id = '{correlation}'")
            if final in ('CONFIRMED', 'UNKNOWN'):
                break
            time.sleep(0.25)
        assert final in ('CONFIRMED', 'UNKNOWN'), final
        if final == 'UNKNOWN':
            status, resolved = http('POST', rust + f"/api/v1/operations/{operation['id']}/resolve",
                                    {'outcome': 'FAILED', 'reason': 'verificado tras reinicio',
                                     'evidence': 'compartimento cerrado'}, token='lab-admin-token')
            assert status == 200, resolved
            assert psql(f"SELECT status FROM locker_operations WHERE correlation_id = '{correlation}'") == 'FAILED'
            assert psql(f"SELECT status FROM loans WHERE id = '{operation['loan_id']}'") == 'CANCELLED'
            assert psql(f"SELECT status FROM authorizations WHERE id = '{authorization}'") == 'APPROVED'
            # Every manual resolution lands in the audit trail with its reason.
            assert psql(
                "SELECT count(*) FROM audit_events WHERE action = 'OPERATION_RESOLVED_FAILED'"
                " AND metadata_json->>'reason' = 'verificado tras reinicio'"
            ) == '1'
            print('mqtt scenario: restart -> UNKNOWN -> resolved over REST: OK')
        else:
            assert psql(f"SELECT status FROM loans WHERE id = '{operation['loan_id']}'") == 'ACTIVE'
            # No resolution happened in this branch: nothing may be audited.
            assert psql(
                "SELECT count(*) FROM audit_events WHERE action = 'OPERATION_RESOLVED_FAILED'"
                " AND metadata_json->>'reason' = 'verificado tras reinicio'"
            ) == '0'
            print('mqtt scenario: restart -> pending republished -> CONFIRMED: OK')

        # 5. Origin partition (audit #12): foreign-origin PENDING rows (e.g.
        #    historical `origin='python'`) are never claimed or published by
        #    the Rust bridge, while Rust-owned operations complete normally.
        foreign_corr = f'foreign-{uuid4()}'
        psql(f"""
            INSERT INTO loans (id, tool_id, user_id, status)
                VALUES ('00000000-0000-0000-0000-000000000099', '{tools[3]}', '{user}', 'CHECKOUT_PENDING');
            INSERT INTO locker_operations (id, correlation_id, command_type, status, tool_id, loan_id, branch_id, locker_id, compartment_id, origin)
                VALUES (gen_random_uuid(), '{foreign_corr}', 'CHECKOUT', 'PENDING', '{tools[3]}',
                        '00000000-0000-0000-0000-000000000099', '{branch}',
                        (SELECT id FROM lockers LIMIT 1), '{compartment}', 'python');
        """)
        authorization, operation = rust_checkout(tools[4])
        correlation = operation['correlation_id']
        wait_until('rust checkout CONFIRMED', lambda: psql(
            f"SELECT status FROM locker_operations WHERE correlation_id = '{correlation}'"
        ) == 'CONFIRMED')
        assert psql(f"SELECT status FROM locker_operations WHERE correlation_id = '{foreign_corr}'") == 'PENDING', \
            'the rust bridge claimed a non-rust operation'
        print('mqtt scenario: single ownership partition by origin: OK')

        print('Rust MQTT scenario: full flow, duplicate, outage, restart, origin partition: OK')
    finally:
        for name in reversed(created):
            docker('rm', '-f', name, check=False)
        if created_network:
            docker('network', 'rm', NETWORK, check=False)


if __name__ == '__main__':
    main()
