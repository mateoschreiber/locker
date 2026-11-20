"""Reproducible performance measurement of the LOCKER panel cycle (F4.2-C).

Disposable stack via compose.rust-preview.yaml under its own project
(locker-rust-bench, port 8086): isolated PostgreSQL, broker and preview
nginx, exactly like the automated smoke but never touching the laboratory
(8083) or the interactive preview (8084). Three steps, run explicitly:

    python scripts/bench_panel.py up
    python scripts/bench_panel.py cycle --minutes 3
    python scripts/bench_panel.py inflate --tools 240 --operations 240 --audit 2400
    python scripts/bench_panel.py cycle --minutes 3
    python scripts/bench_panel.py down

`cycle` replays the panel's polling set through nginx (as the browser
does), in parallel per cycle, and prints a markdown report with per
endpoint latency (avg/p95), body size, requests per minute and docker
stats samples of api-rust and PostgreSQL. Observed values only: anything
derived from code (req/min theoretical) is labeled as such.
"""

import argparse
import concurrent.futures
import json
import os
import subprocess
import sys
import threading
import time
import urllib.request

PROJECT = 'locker-rust-bench'
PORT = int(os.environ.get('LOCKER_BENCH_PORT', '8086'))
BASE = f'http://127.0.0.1:{PORT}'
COMPOSE = ['docker', 'compose', '-p', PROJECT, '-f', 'compose.rust-preview.yaml']

# The panel's polling set (App.tsx `load()` + presence when capabilities
# advertise it): 14 entity lists + dashboard + presence, in parallel every 4 s.
ENTITIES = ['branches', 'roles', 'users', 'memberships', 'lockers',
            'compartments', 'locks', 'cameras', 'tools', 'placements',
            'authorizations', 'loans', 'operations', 'audit']
CYCLE_PATHS = [f'/api/v1/{entity}' for entity in ENTITIES] + [
    '/api/v1/dashboard', '/api/v1/lockers/presence']

# F4.2-B: the per-view dynamic request sets the panel issues in Rust mode
# (reference entities are not polled; administration has no steady cycle).
VIEWS = {
    'inicio': ['/api/v1/dashboard', '/api/v1/operations?attention=1&limit=100',
               '/api/v1/lockers/presence'],
    'operacion': ['/api/v1/authorizations', '/api/v1/loans',
                 '/api/v1/tools?status=AVAILABLE&limit=100'],
    'operaciones': ['/api/v1/operations?limit=100'],
    'inventario': ['/api/v1/tools?status=AVAILABLE&limit=100'],
    'lockers': ['/api/v1/lockers', '/api/v1/lockers/presence'],
    'actividad': ['/api/v1/audit?limit=100', '/api/v1/authorizations',
                  '/api/v1/loans'],
}
CYCLE_SECONDS = 4


def compose(*args, check=True):
    # LOCKER_PREVIEW_PORT drives the compose file's published port; the bench
    # never reuses 8084 (interactive) or 8085 (automated smoke).
    env = {**os.environ, 'LOCKER_PREVIEW_PORT': str(PORT)}
    result = subprocess.run(COMPOSE + list(args), capture_output=True, text=True, env=env)
    if check and result.returncode != 0:
        sys.exit(f'compose {args} failed:\n{result.stdout}{result.stderr}')
    return result


def psql(sql):
    result = compose('exec', '-T', 'postgres', 'psql', '-U', 'locker_rust',
                     '-d', 'locker_rust', '-tAc', sql, check=False)
    return result.stdout.strip()


def wait_until(description, predicate, seconds=120):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(2)
    sys.exit(f'timeout waiting for {description}')


def get(path):
    start = time.perf_counter()
    with urllib.request.urlopen(BASE + path, timeout=10) as response:
        body = response.read()
    return (time.perf_counter() - start) * 1000, len(body)


def cmd_up(_args):
    print(f'building the disposable bench stack (project {PROJECT}, port {PORT})...')
    compose('build')
    compose('up', '-d', 'postgres', 'mosquitto')
    wait_until('isolated PostgreSQL', lambda: compose(
        'exec', '-T', 'postgres', 'pg_isready', '-U', 'locker_rust',
        '-d', 'locker_rust', check=False).returncode == 0)
    wait_until('isolated MQTT', lambda: compose(
        'exec', '-T', 'mosquitto', 'mosquitto_sub', '-h', 'localhost',
        '-t', '$SYS/broker/version', '-C', '1', '-W', '3',
        check=False).returncode == 0)
    # Idempotent: a partially initialized stack (e.g. a failed later step)
    # keeps its schema and seed; the seed itself refuses non-empty databases.
    if psql('SELECT count(*) FROM branches') in ('', '0'):
        compose('run', '--rm', 'init', 'alembic', 'upgrade', 'head')
        compose('run', '--rm', 'init', 'python', '-m', 'app.modules.seed')
    compose('up', '-d', 'simulator', 'api-rust', 'web-rust')
    wait_until('web /health/ready 200', lambda: health_ready())
    print('bench stack ready')


def health_ready():
    try:
        with urllib.request.urlopen(BASE + '/health/ready', timeout=3) as response:
            return response.status == 200
    except OSError:
        return False


def cmd_inflate(args):
    """Grows the disposable preview database only: synthetic rows with the
    same shape as real ones (tools without FKs; audit without actor;
    operations templated on one synthetic loan so every FK holds)."""
    if args.tools:
        before = int(psql('SELECT count(*) FROM tools'))
        psql(f"INSERT INTO tools (id, asset_code, name, description, rfid_tag, status)"
             f" SELECT gen_random_uuid(), 'BENCH-T-'||g||'-'||substr(gen_random_uuid()::text, 1, 8),"
             f" 'Herramienta bench '||g, NULL, NULL, 'AVAILABLE' FROM generate_series(1,{args.tools}) g")
        assert int(psql('SELECT count(*) FROM tools')) == before + args.tools
    if args.audit:
        before = int(psql('SELECT count(*) FROM audit_events'))
        psql(f"INSERT INTO audit_events (id, action, entity_type, entity_id, metadata_json)"
             f" SELECT gen_random_uuid(), 'BENCH_ACTIVITY', 'Tool', 'bench-'||g||'-'||substr(gen_random_uuid()::text, 1, 8), '{{}}'"
             f" FROM generate_series(1,{args.audit}) g")
        assert int(psql('SELECT count(*) FROM audit_events')) == before + args.audit
    if args.operations:
        before = int(psql('SELECT count(*) FROM locker_operations'))
        psql("INSERT INTO authorizations (id, tool_id, user_id, branch_id, status)"
             " SELECT gen_random_uuid(),"
             " (SELECT id FROM tools ORDER BY created_at LIMIT 1),"
             " (SELECT id FROM users WHERE username = 'admin'),"
             " (SELECT id FROM branches ORDER BY created_at LIMIT 1), 'CONSUMED'"
             " WHERE NOT EXISTS (SELECT 1 FROM authorizations WHERE status = 'CONSUMED'"
             " AND tool_id = (SELECT id FROM tools ORDER BY created_at LIMIT 1))")
        psql("INSERT INTO loans (id, tool_id, user_id, authorization_id, status)"
             " SELECT gen_random_uuid(), a.tool_id, a.user_id, a.id, 'RETURNED'"
             " FROM authorizations a WHERE a.status = 'CONSUMED'"
             " AND NOT EXISTS (SELECT 1 FROM loans l WHERE l.authorization_id = a.id)")
        psql(f"INSERT INTO locker_operations (id, correlation_id, command_type, status,"
             f" tool_id, loan_id, branch_id, locker_id, compartment_id, origin, created_at)"
             f" SELECT gen_random_uuid(), 'bench-'||g||'-'||substr(gen_random_uuid()::text, 1, 8), 'CHECKOUT', 'CONFIRMED',"
             f" lo.tool_id, lo.loan_id, (SELECT id FROM branches ORDER BY created_at LIMIT 1),"
             f" (SELECT id FROM lockers ORDER BY created_at LIMIT 1),"
             f" (SELECT id FROM compartments ORDER BY position LIMIT 1), 'bench',"
             f" now() - (g || ' seconds')::interval"
             f" FROM (SELECT l.tool_id, l.id AS loan_id FROM loans l"
             f" JOIN authorizations a ON a.id = l.authorization_id WHERE a.status = 'CONSUMED'"
             f" ORDER BY l.created_at LIMIT 1) lo, generate_series(1,{args.operations}) g")
        assert int(psql('SELECT count(*) FROM locker_operations')) == before + args.operations
    print(f"inflated: tools={args.tools} operations={args.operations} audit={args.audit} "
          f"(rows now: tools={psql('SELECT count(*) FROM tools')}, "
          f"operations={psql('SELECT count(*) FROM locker_operations')}, "
          f"audit={psql('SELECT count(*) FROM audit_events')})")


def docker_stats():
    """One sample of CPU%/RAM for the bench containers, like the F4.0 method."""
    result = subprocess.run(['docker', 'stats', '--no-stream', '--format',
                            '{{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}'],
                            capture_output=True, text=True)
    samples = {}
    for line in result.stdout.splitlines():
        name, cpu, mem = line.split('\t')
        if name.startswith(PROJECT):
            samples[name.replace(f'{PROJECT}-', '').replace('-1', '')] = (cpu, mem)
    return samples


def cmd_cycle(args):
    paths = VIEWS[args.view] if args.view else CYCLE_PATHS
    label = f'vista {args.view}' if args.view else 'ciclo completo del panel (pre-F4.2)'
    samples = {path: [] for path in paths}
    sizes = {path: 0 for path in paths}
    failures = 0
    stop = threading.Event()
    stat_log = []

    def sample_stats():
        # Every 20 s, the F4.0 sampling cadence; one idle sample before cycling.
        stat_log.append(('idle', docker_stats()))
        while not stop.is_set():
            if stop.wait(20):
                break
            stat_log.append(('cycle', docker_stats()))

    sampler = threading.Thread(target=sample_stats)
    sampler.start()
    print(f'cycling {len(paths)} requests every {CYCLE_SECONDS} s '
          f'for {args.minutes} min through nginx ({BASE}) — {label}...')
    deadline = time.monotonic() + args.minutes * 60
    cycles = 0
    while time.monotonic() < deadline:
        with concurrent.futures.ThreadPoolExecutor(len(paths)) as pool:
            futures = {pool.submit(get, path): path for path in paths}
            for future in concurrent.futures.as_completed(futures):
                path = futures[future]
                try:
                    latency, size = future.result()
                except OSError:
                    failures += 1
                    continue
                samples[path].append(latency)
                sizes[path] = size
        cycles += 1
        time.sleep(max(0, CYCLE_SECONDS - (time.monotonic() % CYCLE_SECONDS)))
    stop.set()
    sampler.join()

    print(f'\n### {label} contra Rust (ventana {args.minutes} min, '
          f'{cycles} ciclos, {failures} fallos)\n')
    print(f'- Solicitudes: {cycles * len(paths)} observadas / '
          f'{cycles * len(paths) / args.minutes:.0f} por minuto; '
          f'teóricas {len(paths) / CYCLE_SECONDS * 60:.0f}/min.\n')
    total_bytes = 0
    print('| Endpoint | Promedio (ms) | p95 (ms) | Tamaño |')
    print('|---|---:|---:|---:|')
    for path in paths:
        values = sorted(samples[path])
        if not values:
            print(f'| `{path}` | FALLA | FALLA | — |')
            continue
        p95 = values[min(int(0.95 * (len(values) - 1)), len(values) - 1)]
        total_bytes += sizes[path]
        print(f'| `{path}` | {sum(values) / len(values):.1f} | {p95:.1f} |'
              f' {sizes[path] / 1024:.1f} KB |')
    print(f'\n**~{total_bytes / 1024:.0f} KB por ciclo** (suma de cuerpos sin comprimir), '
          f'~{total_bytes / 1024 * (60 / CYCLE_SECONDS) / 1024:.1f} MB/min.\n')
    print('- `docker stats` (muestra inicial en reposo, luego cada 20 s):')
    for stage, stats in stat_log:
        print(f'  - {stage}: ' + ', '.join(
            f'{name} {cpu} / {mem}' for name, (cpu, mem) in stats.items()))
    if failures:
        sys.exit(f'{failures} requests failed')


def cmd_down(_args):
    compose('down', '-v')
    print('bench stack removed (volumes included)')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('up').set_defaults(func=cmd_up)
    inflate = sub.add_parser('inflate')
    inflate.add_argument('--tools', type=int, default=0)
    inflate.add_argument('--operations', type=int, default=0)
    inflate.add_argument('--audit', type=int, default=0)
    inflate.set_defaults(func=cmd_inflate)
    cycle = sub.add_parser('cycle')
    cycle.add_argument('--minutes', type=int, default=3)
    cycle.add_argument('--view', choices=sorted(VIEWS), default=None,
                       help='cycle only the per-view request set (F4.2-B)')
    cycle.set_defaults(func=cmd_cycle)
    sub.add_parser('down').set_defaults(func=cmd_down)
    args = parser.parse_args()
    args.func(args)


if __name__ == '__main__':
    main()
