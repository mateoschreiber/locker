#!/usr/bin/env python3
"""Smoke-test the Rust image using disposable containers and an isolated database."""

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from uuid import uuid4

image = sys.argv[1] if len(sys.argv) > 1 else 'locker-api-rust'
prefix = f'locker-rust-smoke-{uuid4().hex[:12]}'
network, postgres, native, api = (prefix, f'{prefix}-pg', f'{prefix}-native', f'{prefix}-api')
created_network = False
created = []


def docker(*args, check=True):
    result = subprocess.run(
        ['docker', *args], capture_output=True, text=True, encoding='utf-8', errors='replace',
        check=False, timeout=90,
    )
    if check and result.returncode:
        raise RuntimeError(f'docker {args[0]} failed: {result.stderr.strip()}')
    return result


def wait_until(description, predicate, seconds=45):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.25)
    raise RuntimeError(f'timeout waiting for {description}')


def status(url):
    try:
        with urllib.request.urlopen(url, timeout=5) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code
    except (OSError, urllib.error.URLError):
        return None


def main():
    global created_network
    try:
        docker('network', 'create', network)
        created_network = True
        docker('run', '-d', '--name', postgres, '--network', network, '--network-alias', 'pg',
               '--tmpfs', '/var/lib/postgresql/data', '-e', 'POSTGRES_USER=locker_test',
               '-e', 'POSTGRES_PASSWORD=test_only', '-e', 'POSTGRES_DB=locker_test',
               'postgres:17-alpine')
        created.append(postgres)
        wait_until('isolated PostgreSQL', lambda: docker(
            'exec', postgres, 'pg_isready', '-U', 'locker_test', '-d', 'locker_test',
            check=False).returncode == 0)

        # The bridge requires a broker and /health/ready gates on the events
        # subscription, like the Python bridge.
        mosquitto = f'{prefix}-mqtt'
        docker('run', '-d', '--name', mosquitto, '--network', network, '--network-alias', 'mqtt',
               '-v', f'{os.getcwd()}/infra/docker/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro',
               'eclipse-mosquitto:2.0.22')
        created.append(mosquitto)
        wait_until('isolated MQTT', lambda: docker(
            'exec', mosquitto, 'mosquitto_sub', '-h', 'localhost', '-t', '$SYS/broker/version',
            '-C', '1', '-W', '3', check=False).returncode == 0)

        url = 'postgres://locker_test:test_only@pg:5432/locker_test'
        docker('run', '-d', '--name', native, '--network', 'none', '-e', 'RUST_LOG=info',
               '-e', f'RUST_DATABASE_URL={url}', '-e', 'RUST_MQTT_ENABLED=false', image)
        created.append(native)

        def native_bound():
            logs = docker('logs', native, check=False).stdout
            for line in logs.splitlines():
                try:
                    if json.loads(line).get('fields', {}).get('address') == '127.0.0.1:8001':
                        return True
                except json.JSONDecodeError:
                    continue
            return False

        wait_until('native default loopback bind', native_bound)
        listeners = docker('exec', native, 'cat', '/proc/net/tcp').stdout
        assert '0100007F:1F41' in listeners and '00000000:1F41' not in listeners, listeners
        docker('stop', '-t', '10', native)
        assert docker('inspect', '-f', '{{.State.ExitCode}}', native).stdout.strip() == '0'

        for invalid in ('0', '31'):
            result = docker('run', '--rm', '--network', 'none', '-e', f'RUST_DATABASE_URL={url}',
                            '-e', f'RUST_DB_TIMEOUT_SECONDS={invalid}', image, check=False)
            assert result.returncode != 0 and 'between 1 and 30' in result.stderr

        docker('run', '-d', '--name', api, '--network', network, '-p', '127.0.0.1::8001',
               '-e', f'RUST_DATABASE_URL={url}', '-e', 'RUST_BIND_ADDRESS=0.0.0.0:8001',
               '-e', 'RUST_DB_TIMEOUT_SECONDS=3', '-e', 'RUST_MQTT_HOST=mqtt', image)
        created.append(api)
        port = docker('inspect', '-f', '{{(index (index .NetworkSettings.Ports "8001/tcp") 0).HostPort}}',
                      api).stdout.strip()
        base = f'http://127.0.0.1:{port}'
        wait_until('Rust /health/live 200', lambda: status(base + '/health/live') == 200)
        wait_until('Rust /health/ready 200 (MQTT subscribed)', lambda: status(base + '/health/ready') == 200)
        # Losing the broker must take readiness down, mirroring the Python bridge.
        docker('stop', '-t', '3', mosquitto)
        wait_until('Rust /health/ready 503 without broker', lambda: status(base + '/health/ready') == 503)
        assert status(base + '/health/live') == 200
        docker('start', mosquitto)
        wait_until('Rust /health/ready 200 after broker recovery', lambda: status(base + '/health/ready') == 200)
        docker('stop', '-t', '10', postgres)
        wait_until('Rust /health/ready 503', lambda: status(base + '/health/ready') == 503)
        assert status(base + '/health/live') == 200
        docker('stop', '-t', '10', api)
        assert docker('inspect', '-f', '{{.State.ExitCode}}', api).stdout.strip() == '0'
        print('Rust Docker smoke: loopback default, 200/200, MQTT gate, PostgreSQL 503, '
              'graceful stop: OK')
    finally:
        for name in reversed(created):
            docker('rm', '-f', name, check=False)
        if created_network:
            docker('network', 'rm', network, check=False)


if __name__ == '__main__':
    main()
