#!/usr/bin/env python3
"""Exercise the real API container with a disposable Compose project."""

import json
import subprocess
import sys
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
PROJECT = f'locker-restart-{uuid4().hex[:12]}'
COMPOSE = ['docker', 'compose', '-p', PROJECT, '-f', 'compose.test.yaml', '--profile', 'restart']


def run(*args, input_text=None):
    result = subprocess.run(
        [*COMPOSE, *args], cwd=ROOT, text=True, input=input_text, check=False,
        capture_output=True, timeout=180, encoding='utf-8', errors='replace',
    )
    if result.returncode:
        raise RuntimeError(f"{' '.join(args)} failed:\n{result.stdout}\n{result.stderr}")
    return result.stdout.strip()


def main():
    try:
        run('up', '-d', '--build', '--wait', '--wait-timeout', '90', 'api', 'simulator')
        run('build', 'tests')
        run('run', '--rm', '--no-deps', '-T', 'tests', 'python', 'tests/docker_scenario.py', 'empty')
        run('exec', '-T', 'api', 'python', '-m', 'app.modules.seed')
        before = run('run', '--rm', '--no-deps', '-T', 'tests', 'python',
                     'tests/docker_scenario.py', 'before')
        json.loads(before)
        container = run('ps', '-q', 'api')
        started = subprocess.check_output(
            ['docker', 'inspect', '-f', '{{.State.StartedAt}}', container], text=True,
        ).strip()
        run('restart', 'api')
        restarted = subprocess.check_output(
            ['docker', 'inspect', '-f', '{{.State.StartedAt}}', container], text=True,
        ).strip()
        assert started != restarted, 'API container did not restart'
        print(run('run', '--rm', '--no-deps', '-T', 'tests', 'python',
                  'tests/docker_scenario.py', 'after', input_text=before))
    except Exception:
        print(run('logs', '--tail', '80', 'api', 'simulator', 'mosquitto'), file=sys.stderr)
        raise
    finally:
        run('down', '-v', '--remove-orphans')


if __name__ == '__main__':
    main()
